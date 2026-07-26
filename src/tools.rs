//! How each built-in tool's call and result are set.
//!
//! A call's summary line carries the labelling and its body carries the
//! subject, so the views here decide two things per tool: what the line says,
//! and what shape the subject takes (a shell command, a diff, a plan, a list of
//! questions). The shapes are the harness's rather than a contract, so a call
//! whose input doesn't match what its view expects falls back to
//! pretty-printed JSON: a tool that grows a field must not break a render.

use std::{borrow::Cow, path::Path};

use maud::{Markup, html};
use serde_json::Value;

use crate::{
    render::{Scribe, json},
    transcript::{Answered, Block, Known, ToolResultContent},
};

/// How one tool call is set: what its summary line says, and what its fold
/// holds. A call whose subject the summary already states in full has no body,
/// and is set flat with no fold to open.
pub struct Setting {
    pub gist: Option<String>,
    /// Where the gist points, when the call's subject is a URL.
    pub href: Option<String>,
    /// Qualifiers: options that change what the call does, which the subject
    /// its body shows doesn't say.
    pub notes: Vec<String>,
    pub body: Option<Markup>,
}

impl Setting {
    pub(crate) fn new() -> Self {
        Self {
            gist: None,
            href: None,
            notes: Vec::new(),
            body: None,
        }
    }

    pub(crate) fn body(mut self, body: Markup) -> Self {
        self.body = Some(body);
        self
    }

    pub(crate) fn gist(mut self, gist: impl Into<String>) -> Self {
        self.gist = Some(gist.into());
        self
    }

    fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }

    pub(crate) fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub(crate) fn maybe_gist(self, gist: Option<impl Into<String>>) -> Self {
        match gist {
            Some(gist) => self.gist(gist),
            None => self,
        }
    }

    pub(crate) fn maybe_note(self, note: Option<impl Into<String>>) -> Self {
        match note {
            Some(note) => self.note(note),
            None => self,
        }
    }
}

/// How a call is set, falling back to JSON for a tool with no view of its own
/// and for one whose input doesn't match the shape its view expects.
pub fn call(scribe: &Scribe, name: &str, input: &Value) -> Setting {
    view(scribe, name, input)
        .unwrap_or_else(|| Setting::new().maybe_gist(subject(input)).body(json(input)))
}

fn view(scribe: &Scribe, name: &str, input: &Value) -> Option<Setting> {
    match name {
        "Bash" => bash(scribe, input),
        "Read" => read(input),
        "Write" => write(scribe, input),
        "Edit" => edit(scribe, input),
        "TodoWrite" => todos(input),
        "Agent" => agent(scribe, input),
        "Skill" => skill(scribe, input),
        "ToolSearch" => tool_search(input),
        "WebSearch" => web_search(input),
        "WebFetch" => web_fetch(scribe, input),
        "TaskCreate" | "TaskUpdate" => task_write(scribe, input),
        "TaskGet" | "TaskOutput" | "TaskStop" => task_reference(input),
        "AskUserQuestion" => questions(scribe, input),
        "EnterPlanMode" => Some(Setting::new()),
        "ExitPlanMode" => plan(scribe, input),
        "Workflow" => workflow(scribe, input),
        "SendMessage" => message(scribe, input),
        "ReportFindings" => findings(input),
        _ => None,
    }
}

/// A one-line summary for a tool with no view of its own, drawn from whichever
/// field carries the subject of the call. A call that describes itself is taken
/// at its word: the description says what the call is *for*, which reads better
/// folded than the command or prompt it stands in front of, and the body shows
/// that anyway.
fn subject(input: &Value) -> Option<&str> {
    [
        "description",
        "command",
        "file_path",
        "pattern",
        "path",
        "url",
        "query",
        "prompt",
    ]
    .iter()
    .find_map(|field| text(input, field))
}

fn text<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input.get(field)?.as_str()
}

/// A body that is written to be read: a prompt, a plan, a message, a report.
/// Markdown is what these are composed in, so markdown is how they are set.
pub(crate) fn prose(scribe: &Scribe, source: &str) -> Markup {
    html! { div .tool.tool--prose { (scribe.markdown(source)) } }
}

fn flag(input: &Value, field: &str) -> bool {
    input.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn bash(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let command = text(input, "command")?;
    Some(
        Setting::new()
            .gist(text(input, "description").unwrap_or_else(|| first_line(command)))
            .maybe_note(flag(input, "run_in_background").then_some("background"))
            .maybe_note(input.get("timeout").and_then(Value::as_u64).map(duration))
            .maybe_note(flag(input, "dangerouslyDisableSandbox").then_some("sandbox off"))
            .body(scribe.code_block("bash", command)),
    )
}

/// A read has no body: the file it names and the lines it took are the whole
/// call, and both fit on the summary line. The contents show up in the result.
fn read(input: &Value) -> Option<Setting> {
    let path = text(input, "file_path")?;
    let offset = input.get("offset").and_then(Value::as_u64);
    let limit = input.get("limit").and_then(Value::as_u64);
    Some(Setting::new().gist(path).maybe_note(span(offset, limit)))
}

/// Which lines of a file a read asked for, or `None` when it asked for all
/// of them, or when the numbers it asked with describe no span at all (a limit
/// of zero takes nothing, and a limit that runs past the end of the counting
/// names no last line).
fn span(offset: Option<u64>, limit: Option<u64>) -> Option<String> {
    match (offset, limit) {
        (Some(offset), Some(limit)) => {
            let last = offset.checked_add(limit.checked_sub(1)?)?;
            Some(format!("lines {offset}–{last}"))
        }
        (Some(offset), None) => Some(format!("from line {offset}")),
        (None, Some(0)) => None,
        (None, Some(limit)) => Some(format!("first {limit} lines")),
        (None, None) => None,
    }
}

fn write(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let path = text(input, "file_path")?;
    let content = text(input, "content")?;
    Some(
        Setting::new()
            .gist(path)
            .body(scribe.code_block(lang_for_path(path), content)),
    )
}

fn edit(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let old = text(input, "old_string")?;
    let new = text(input, "new_string")?;
    Some(
        Setting::new()
            .maybe_gist(text(input, "file_path"))
            .maybe_note(flag(input, "replace_all").then_some("replace all"))
            .body(scribe.code_block("diff", &unified_diff(old, new))),
    )
}

/// A checklist, labelled by whichever item is being worked: that is what the
/// list was written to say.
fn todos(input: &Value) -> Option<Setting> {
    let todos = input.get("todos")?.as_array()?;
    let active = todos
        .iter()
        .find(|todo| text(todo, "status") == Some("in_progress"))
        .and_then(|todo| text(todo, "content"));
    Some(Setting::new().maybe_gist(active).body(html! {
        ul .tool.tool--todos {
            @for todo in todos {
                @let content = text(todo, "content").unwrap_or("");
                @let status = text(todo, "status").unwrap_or("pending");
                li .tool__todo data-status=(status) { (content) }
            }
        }
    }))
}

fn agent(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let prompt = text(input, "prompt")?;
    Some(
        Setting::new()
            .maybe_gist(text(input, "description"))
            .maybe_note(text(input, "subagent_type"))
            .maybe_note(text(input, "model"))
            .maybe_note(text(input, "isolation"))
            .maybe_note(flag(input, "run_in_background").then_some("background"))
            .body(prose(scribe, prompt)),
    )
}

fn skill(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let skill = text(input, "skill")?;
    let setting = Setting::new().gist(skill);
    Some(match text(input, "args") {
        Some(args) => setting.body(prose(scribe, args)),
        None => setting,
    })
}

fn tool_search(input: &Value) -> Option<Setting> {
    let query = text(input, "query")?;
    Some(
        Setting::new().gist(query).maybe_note(
            input
                .get("max_results")
                .and_then(Value::as_u64)
                .map(|most| format!("at most {most}")),
        ),
    )
}

fn web_search(input: &Value) -> Option<Setting> {
    Some(Setting::new().gist(text(input, "query")?))
}

fn web_fetch(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let url = text(input, "url")?;
    let prompt = text(input, "prompt")?;
    Some(
        Setting::new()
            .gist(url)
            .href(url)
            .body(prose(scribe, prompt)),
    )
}

fn task_write(scribe: &Scribe, input: &Value) -> Option<Setting> {
    // A create names its task; an update names only the id it changes.
    let gist = match text(input, "subject") {
        Some(subject) => subject.to_owned(),
        None => format!("task {}", text(input, "taskId")?),
    };
    let setting = Setting::new().gist(gist).maybe_note(text(input, "status"));
    Some(match text(input, "description") {
        Some(description) => setting.body(prose(scribe, description)),
        None => setting,
    })
}

fn task_reference(input: &Value) -> Option<Setting> {
    let id = text(input, "taskId").or_else(|| text(input, "task_id"))?;
    Some(
        Setting::new()
            .gist(id)
            .maybe_note(flag(input, "block").then_some("waits"))
            .maybe_note(input.get("timeout").and_then(Value::as_u64).map(duration)),
    )
}

fn questions(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let asked = input.get("questions")?.as_array()?;
    let first = asked
        .first()
        .and_then(|question| text(question, "question"));
    Some(
        Setting::new()
            .maybe_gist(first)
            .maybe_note(
                asked
                    .iter()
                    .any(|question| flag(question, "multiSelect"))
                    .then_some("multi-select"),
            )
            .body(html! {
                div .tool.tool--questions {
                    @for question in asked {
                        section .tool__question {
                            p .tool__ask {
                                @if let Some(header) = text(question, "header") {
                                    span .tool__header { (header) }
                                }
                                (text(question, "question").unwrap_or(""))
                            }
                            @if let Some(options) = question.get("options").and_then(Value::as_array) {
                                ul .tool__options {
                                    @for option in options {
                                        li .tool__option {
                                            span .tool__label { (text(option, "label").unwrap_or("")) }
                                            @if let Some(description) = text(option, "description") {
                                                span .tool__description { (description) }
                                            }
                                            // An option can carry a mockup of
                                            // what choosing it would produce,
                                            // which is what the reader actually
                                            // compared, so it is shown as the
                                            // laid-out thing it is.
                                            @if let Some(preview) = text(option, "preview") {
                                                (scribe.code_block("text", preview))
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }),
    )
}

/// A plan is a markdown document, so it is set as one. The prompts it asks to
/// be pre-approved sit under it, since they are what the plan will run rather
/// than part of what it says.
fn plan(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let plan = text(input, "plan")?;
    let allowed = input
        .get("allowedPrompts")
        .and_then(Value::as_array)
        .filter(|prompts| !prompts.is_empty());
    Some(Setting::new().maybe_gist(heading(plan)).body(html! {
        (prose(scribe, plan))
        @if let Some(allowed) = allowed {
            ul .tool.tool--prompts {
                @for prompt in allowed {
                    li .tool__prompt {
                        span .tool__label { (text(prompt, "tool").unwrap_or("")) }
                        (text(prompt, "prompt").unwrap_or(""))
                    }
                }
            }
        }
    }))
}

fn workflow(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let script = text(input, "script");
    let path = text(input, "scriptPath");
    let name = text(input, "name");
    let gist = name
        .or(path)
        .map(str::to_owned)
        .or_else(|| script.and_then(workflow_name))?;
    let setting = Setting::new()
        .gist(gist)
        .maybe_note(text(input, "resumeFromRunId").map(|_| "resumed"));
    Some(match (script, input.get("args")) {
        (Some(script), _) => setting.body(scribe.code_block("javascript", script)),
        (None, Some(args)) => setting.body(json(args)),
        (None, None) => setting,
    })
}

/// The `name` a workflow script declares in its `meta` block, which says what
/// the script is where a path or a name field doesn't.
fn workflow_name(script: &str) -> Option<String> {
    let (_, rest) = script.split_once("name:")?;
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let (name, _) = rest[quote.len_utf8()..].split_once(quote)?;
    Some(name.to_owned())
}

/// A message to another agent. The harness records the recipient and the body
/// twice under different names; one of each is the whole message.
fn message(scribe: &Scribe, input: &Value) -> Option<Setting> {
    let body = text(input, "message").or_else(|| text(input, "content"))?;
    Some(
        Setting::new()
            .maybe_gist(text(input, "summary"))
            .maybe_note(text(input, "to").or_else(|| text(input, "recipient")))
            .body(prose(scribe, body)),
    )
}

fn findings(input: &Value) -> Option<Setting> {
    let found = input.get("findings")?.as_array()?;
    Some(
        Setting::new()
            .gist(match found.len() {
                1 => "1 finding".to_owned(),
                count => format!("{count} findings"),
            })
            .maybe_note(text(input, "level"))
            .body(html! {
                ul .tool.tool--findings {
                    @for finding in found {
                        li .tool__finding {
                            p .tool__where {
                                span .tool__label { (text(finding, "file").unwrap_or("")) }
                                @if let Some(line) = finding.get("line").and_then(Value::as_u64) {
                                    span .tool__line { ":" (line) }
                                }
                                @if let Some(category) = text(finding, "category") {
                                    span .tool__header { (category) }
                                }
                                @if let Some(verdict) = text(finding, "verdict") {
                                    span .tool__header { (verdict) }
                                }
                            }
                            p .tool__summary { (text(finding, "summary").unwrap_or("")) }
                            @if let Some(scenario) = text(finding, "failure_scenario") {
                                p .tool__scenario { (scenario) }
                            }
                        }
                    }
                }
            }),
    )
}

/// How a result is set, which takes the call it answers: the wire format says
/// only that some text came back, and what that text *is* (a file, a search, a
/// terminal's output) is a fact about the tool that produced it.
pub fn result(
    scribe: &Scribe,
    answers: Option<&Answered>,
    content: &ToolResultContent,
    is_error: bool,
) -> Markup {
    let tool = answers.map(|answered| answered.tool.as_str());
    let text = match spoken(content) {
        Ok(text) => text,
        Err(blocks) => return blocks_result(scribe, tool, blocks),
    };
    let text = text.as_ref();
    if is_error {
        return failure(text);
    }
    match tool {
        Some("Read") => source(scribe, answers.and_then(|a| a.subject.as_deref()), text),
        Some("WebSearch") => web_results(scribe, text),
        Some("TaskOutput") => task_output(scribe, text),
        Some("AskUserQuestion") => chosen(scribe, text),
        // Output written to be read as prose, so it is set as prose.
        Some("Agent" | "ExitPlanMode" | "Skill" | "WebFetch") => prose(scribe, text),
        _ => plain(scribe, text),
    }
}

/// What a tool answers when it has nothing to say beyond having done the thing:
/// a prefix, and the suffix the sentence ends on where the subject sits in the
/// middle of it. The parenthetical a file write appends is stripped first, so
/// it needn't appear here.
///
/// The match is on the sentence rather than on the tool, because these results
/// are not uniformly empty: an edit that also warns the file changed on disk
/// says something a reader needs, and must not be swallowed with the rest.
const ACKNOWLEDGEMENTS: [(&str, &str, &str); 9] = [
    ("Write", "File created successfully at:", ""),
    ("Write", "The file", "has been updated successfully."),
    ("Edit", "The file", "has been updated successfully."),
    (
        "Edit",
        "The file",
        "All occurrences were successfully replaced.",
    ),
    ("TodoWrite", "Todos have been modified successfully.", ""),
    ("TaskUpdate", "Updated task #", ""),
    ("Skill", "Launching skill:", ""),
    // A background agent answers with the id and output file the model needs to
    // reach it again, and says so in as many words: none of it is addressed to
    // a reader, and the call above it already shows what the agent was sent.
    ("Agent", "Async agent launched successfully.", ""),
    ("EnterPlanMode", "Entered plan mode.", ""),
];

/// The note the harness appends to a file write, addressed to the model rather
/// than to anyone reading the folio.
const FILE_STATE_NOTE: &str = "(file state is current in your context — no need to Read it back)";

/// True when a result says only that the call above it was carried out. The
/// call already shows the file it wrote or the task it changed, so the folio
/// drops these rather than answering every edit with a line saying it worked.
pub fn acknowledges(tool: &str, text: &str) -> bool {
    let text = text
        .trim()
        .strip_suffix(FILE_STATE_NOTE)
        .unwrap_or(text)
        .trim();
    ACKNOWLEDGEMENTS
        .iter()
        .filter(|(named, _, _)| *named == tool)
        .any(|(_, opening, closing)| text.starts_with(opening) && text.ends_with(closing))
}

/// The two ways the harness introduces what a reader chose, and the sentence it
/// may close with. Both are addressed to the model rather than to a reader.
const ANSWER_OPENINGS: [&str; 2] = ["Your questions have been answered: ", "The user answered: "];

const ANSWER_CLOSINGS: [&str; 2] = [
    ". You can now continue with these answers in mind.",
    " You can now continue with these answers in mind.",
];

/// Where an answered option's own preview is echoed back after its label.
const SELECTED_PREVIEW: &str = "\" selected preview:";

/// How the harness introduces text the reader typed instead of choosing one of
/// the options. What follows is the answer; this is the frame around it.
const TYPED_OPENING: &str = "(no option selected) notes:";

/// One settled question: what was asked, what came back, and whether the reader
/// typed it rather than taking one of the options offered.
struct Answer<'a> {
    question: &'a str,
    chosen: &'a str,
    typed: bool,
}

/// What was chosen, against what was asked. The harness answers as one long
/// sentence naming each question back before its answer, so this recovers the
/// pairs; the answer is what a reader wants, and the sentence buries it.
///
/// The pairs are found by anchoring on the `"=` between a question and its
/// answer and then walking back from the *next* anchor to the `, "` that opens
/// the following question. Splitting forward on the separators instead breaks
/// on real transcripts: a question quotes code containing quotes, a free-text
/// answer runs to several clauses, and an option that carried a preview has it
/// echoed inline, all of which put the delimiters inside the values.
fn answers(text: &str) -> Option<Vec<Answer<'_>>> {
    let text = text.trim();
    let mut body = ANSWER_OPENINGS
        .iter()
        .find_map(|opening| text.strip_prefix(opening))?;
    body = ANSWER_CLOSINGS
        .iter()
        .find_map(|closing| body.strip_suffix(closing))
        .unwrap_or(body);
    if !body.starts_with('"') {
        return None;
    }

    let anchors: Vec<usize> = body.match_indices("\"=").map(|(at, _)| at).collect();
    let mut pairs = Vec::with_capacity(anchors.len());
    // Past the quote that opens the first question.
    let mut cursor = 1;
    for (index, &anchor) in anchors.iter().enumerate() {
        let question = body.get(cursor..anchor)?;
        let mut start = anchor + 2;
        // An answer is quoted where an option was chosen, and bare where the
        // reader typed something instead.
        let quoted = body.get(start..)?.starts_with('"');
        if quoted {
            start += 1;
        }
        let (region, next) = match anchors.get(index + 1) {
            Some(&following) => {
                let opens = body.get(start..following)?.rfind(", \"")? + start;
                (body.get(start..opens)?, opens + 3)
            }
            None => (body.get(start..)?, body.len()),
        };
        // The preview is already shown against the option it belongs to, in the
        // call above, so only the label it followed is kept.
        let answer = match region.find(SELECTED_PREVIEW) {
            Some(echo) => region.get(..echo)?,
            None if quoted => region.strip_suffix('"').unwrap_or(region),
            None => region,
        };
        let answer = answer.trim();
        pairs.push(Answer {
            question: question.trim(),
            chosen: answer.strip_prefix(TYPED_OPENING).unwrap_or(answer).trim(),
            typed: !quoted,
        });
        cursor = next;
    }
    (!pairs.is_empty()).then_some(pairs)
}

/// What a reader chose. A result that answers nothing (the harness reports a
/// question that timed out this way) has no pairs to find, and stands as the
/// note it is.
fn chosen(scribe: &Scribe, text: &str) -> Markup {
    let Some(answered) = answers(text) else {
        return plain(scribe, text);
    };
    html! {
        ul .tool.tool--answers {
            @for answer in answered {
                li .tool__answer {
                    p .tool__ask { (answer.question) }
                    // A typed answer is marked as one: it took none of the
                    // options above it, which is itself part of what was said.
                    p .tool__chosen data-typed[answer.typed] { (answer.chosen) }
                }
            }
        }
    }
}

/// A failure is a diagnostic, so it is shown as it came, less the tag the
/// harness wraps a tool's own error in.
fn failure(text: &str) -> Markup {
    let text = text
        .trim()
        .strip_prefix("<tool_use_error>")
        .and_then(|text| text.strip_suffix("</tool_use_error>"))
        .unwrap_or(text);
    // A failing command's diagnostics are where colour carries the most: it is
    // what the tool used to mark the failure in the first place.
    terminal(text)
}

/// Output with no shape of its own: pretty-printed where it is JSON (several
/// tools answer with a JSON object on one line), and otherwise as the terminal
/// wrote it, colour and all.
pub(crate) fn plain(scribe: &Scribe, text: &str) -> Markup {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(value) if value.is_object() || value.is_array() => scribe.code_block(
            "json",
            &serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_owned()),
        ),
        _ => terminal(text),
    }
}

/// The eight colours a terminal names, in the order ANSI numbers them. The
/// stylesheet grinds each into one of the folio's own pigments, so a build log
/// reads in the same palette as everything around it (see the classes-not-colors
/// invariant in CLAUDE.md).
const COLOURS: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

const BRIGHT_COLOURS: [&str; 8] = [
    "bright-black",
    "bright-red",
    "bright-green",
    "bright-yellow",
    "bright-blue",
    "bright-magenta",
    "bright-cyan",
    "bright-white",
];

/// A colour a terminal asked for. The sixteen it *names* are the folio's to
/// grind, so they resolve to a class and the stylesheet picks the pigment; a
/// colour given outright as channels is the tool's own choice and no palette
/// token can stand for it, so it is carried as the value it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pigment {
    Named(&'static str),
    Exact(u8, u8, u8),
}

impl Pigment {
    fn hex(self) -> Option<String> {
        match self {
            Pigment::Named(_) => None,
            Pigment::Exact(red, green, blue) => Some(format!("#{red:02x}{green:02x}{blue:02x}")),
        }
    }

    fn name(self) -> Option<&'static str> {
        match self {
            Pigment::Named(name) => Some(name),
            Pigment::Exact(..) => None,
        }
    }
}

/// How a terminal was writing when it wrote a run of its output. Tool results
/// carry the escape codes verbatim, and a test run or a build log says what
/// passed and what failed in green and red: dropping that leaves a reader with
/// an undifferentiated wall, and showing it raw leaves them with the escapes.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Ink {
    foreground: Option<Pigment>,
    background: Option<Pigment>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
}

impl Ink {
    /// The classes this ink is drawn with, or `None` when it names nothing the
    /// stylesheet owns.
    fn classes(self) -> Option<String> {
        let mut classes = Vec::new();
        if let Some(name) = self.foreground.and_then(Pigment::name) {
            classes.push(format!("ansi--{name}"));
        }
        if let Some(name) = self.background.and_then(Pigment::name) {
            classes.push(format!("ansi--bg-{name}"));
        }
        for (set, name) in [
            (self.bold, "bold"),
            (self.dim, "dim"),
            (self.italic, "italic"),
            (self.underline, "underline"),
        ] {
            if set {
                classes.push(format!("ansi--{name}"));
            }
        }
        (!classes.is_empty()).then(|| format!("ansi {}", classes.join(" ")))
    }

    /// The colours this ink states outright, which no class can stand for.
    fn style(self) -> Option<String> {
        let mut declarations = Vec::new();
        if let Some(hex) = self.foreground.and_then(Pigment::hex) {
            declarations.push(format!("color:{hex}"));
        }
        if let Some(hex) = self.background.and_then(Pigment::hex) {
            declarations.push(format!("background-color:{hex}"));
        }
        (!declarations.is_empty()).then(|| declarations.join(";"))
    }

    /// Applies one `ESC[...m` sequence's parameters.
    fn apply(&mut self, params: &str) {
        // An omitted parameter means zero, which is the reset.
        let mut codes = params
            .split(';')
            .map(|param| param.trim().parse::<u16>().unwrap_or(0));
        while let Some(code) = codes.next() {
            match code {
                0 => *self = Self::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                22 => (self.bold, self.dim) = (false, false),
                23 => self.italic = false,
                24 => self.underline = false,
                30..=37 => self.foreground = Some(Pigment::Named(COLOURS[code as usize - 30])),
                38 => self.foreground = extended(&mut codes),
                39 => self.foreground = None,
                40..=47 => self.background = Some(Pigment::Named(COLOURS[code as usize - 40])),
                48 => self.background = extended(&mut codes),
                49 => self.background = None,
                90..=97 => {
                    self.foreground = Some(Pigment::Named(BRIGHT_COLOURS[code as usize - 90]));
                }
                100..=107 => {
                    self.background = Some(Pigment::Named(BRIGHT_COLOURS[code as usize - 100]));
                }
                _ => {}
            }
        }
    }
}

/// The colour an extended sequence asks for, taken from the parameters after
/// the `38` or `48` that introduced it: `5;n` picks one of the 256, and
/// `2;r;g;b` gives the channels outright.
fn extended(codes: &mut impl Iterator<Item = u16>) -> Option<Pigment> {
    match codes.next()? {
        5 => Some(indexed(u8::try_from(codes.next()?).ok()?)),
        2 => {
            let mut channel = || u8::try_from(codes.next()?).ok();
            Some(Pigment::Exact(channel()?, channel()?, channel()?))
        }
        _ => None,
    }
}

/// One of the 256 colours by index: the sixteen the terminal names, then a
/// 6x6x6 cube, then a 24-step grey ramp. Only the first sixteen are the
/// folio's to reinterpret; the rest are exact points the tool chose.
fn indexed(index: u8) -> Pigment {
    match index {
        0..=7 => Pigment::Named(COLOURS[index as usize]),
        8..=15 => Pigment::Named(BRIGHT_COLOURS[index as usize - 8]),
        16..=231 => {
            let cube = index - 16;
            // The cube's six steps are not evenly spaced: the first is black,
            // and the rest run from 95 up in steps of 40.
            let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            Pigment::Exact(level(cube / 36), level((cube / 6) % 6), level(cube % 6))
        }
        _ => {
            let grey = 8 + (index - 232) * 10;
            Pigment::Exact(grey, grey, grey)
        }
    }
}

/// A terminal's output with its colour kept: each run of text in the ink that
/// was in force when it was written.
fn terminal(text: &str) -> Markup {
    html! {
        pre { code {
            @for (ink, run) in ansi_runs(text) {
                @let classes = ink.classes();
                @let style = ink.style();
                @if classes.is_some() || style.is_some() {
                    span class=[classes] style=[style] { (run) }
                } @else {
                    (run)
                }
            }
        } }
    }
}

/// Splits output into runs of text, each with the ink it was written in. Every
/// escape sequence is consumed: the colour ones change the ink, and the rest
/// (cursor moves, erases) are the terminal being driven rather than anything to
/// show, so they leave nothing behind.
fn ansi_runs(text: &str) -> Vec<(Ink, &str)> {
    let mut runs = Vec::new();
    let mut ink = Ink::default();
    let mut rest = text;
    while let Some(escape) = rest.find('\u{1b}') {
        let (written, from_escape) = rest.split_at(escape);
        if !written.is_empty() {
            runs.push((ink, written));
        }
        match control_sequence(from_escape) {
            Some((params, 'm', remainder)) => {
                ink.apply(params);
                rest = remainder;
            }
            Some((_, _, remainder)) => rest = remainder,
            // A lone escape drives nothing, so it simply goes.
            None => rest = &from_escape[1..],
        }
    }
    if !rest.is_empty() {
        runs.push((ink, rest));
    }
    runs
}

/// The `ESC[<params><final>` sequence at the start of `text`, as its
/// parameters, its final byte, and what follows it.
fn control_sequence(text: &str) -> Option<(&str, char, &str)> {
    let params = text.strip_prefix("\u{1b}[")?;
    // Parameter and intermediate bytes run up to the final byte, which is the
    // first in the range @ to ~ and is what says which sequence this is.
    let end = params.find(|byte: char| ('\u{40}'..='\u{7e}').contains(&byte))?;
    let (params, rest) = params.split_at(end);
    let mut characters = rest.chars();
    let final_byte = characters.next()?;
    Some((params, final_byte, characters.as_str()))
}

/// A file's contents, set the way the file itself would be. The harness numbers
/// the lines it returns; those numbers are the harness talking, not the file,
/// so they come off before the file is highlighted by its extension.
fn source(scribe: &Scribe, path: Option<&str>, text: &str) -> Markup {
    let lang = path.map_or("", lang_for_path);
    scribe.code_block(lang, &unnumbered(text))
}

/// A numbered listing with its numbering taken off, or the text unchanged when
/// it isn't one: a file whose own lines start with numbers must not lose them.
fn unnumbered(text: &str) -> String {
    let stripped: Option<Vec<&str>> = text.lines().map(strip_line_number).collect();
    stripped.map_or_else(|| text.to_owned(), |lines| lines.join("\n"))
}

/// One listing line without its `<spaces><number><tab>` prefix. A blank line
/// carries no number and stands for itself.
fn strip_line_number(line: &str) -> Option<&str> {
    if line.trim().is_empty() {
        return Some(line);
    }
    let (number, rest) = line.trim_start().split_once('\t')?;
    number.parse::<u64>().ok().map(|_| rest)
}

/// A web search answers with a line of JSON links between two of prose. The
/// links are the result, so they are set as links.
fn web_results(scribe: &Scribe, text: &str) -> Markup {
    let Some((before, rest)) = text.split_once("Links: ") else {
        return plain(scribe, text);
    };
    let (links, after) = rest.split_once('\n').unwrap_or((rest, ""));
    let Ok(Value::Array(links)) = serde_json::from_str::<Value>(links.trim()) else {
        return plain(scribe, text);
    };
    html! {
        div .tool.tool--results {
            p .tool__query { (before.trim()) }
            ul .tool__links {
                @for link in &links {
                    @if let Some(url) = text_of(link, "url") {
                        li { a href=(url) { (text_of(link, "title").unwrap_or(url)) } }
                    }
                }
            }
            (prose(scribe, after.trim()))
        }
    }
}

fn text_of<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field)?.as_str()
}

/// A background task answers with its status in pseudo-XML tags and its work in
/// an `output` tag: the status is a handful of facts and the output is prose.
fn task_output(scribe: &Scribe, text: &str) -> Markup {
    let tagged: Vec<(&str, &str)> = tags(text);
    if tagged.is_empty() {
        return plain(scribe, text);
    }
    html! {
        dl .tool.tool--facts {
            @for (name, value) in tagged.iter().filter(|(name, _)| *name != "output") {
                dt { (name) }
                dd { (value) }
            }
        }
        @for (_, output) in tagged.iter().filter(|(name, _)| *name == "output") {
            (prose(scribe, output))
        }
    }
}

/// The `<tag>value</tag>` pairs in a harness answer, in the order they appear.
fn tags(text: &str) -> Vec<(&str, &str)> {
    let mut tagged = Vec::new();
    let mut rest = text;
    while let Some((_, after)) = rest.split_once('<') {
        let Some((name, after)) = after.split_once('>') else {
            break;
        };
        let close = format!("</{name}>");
        let Some((value, after)) = after.split_once(close.as_str()) else {
            rest = after;
            continue;
        };
        tagged.push((name, value.trim()));
        rest = after;
    }
    tagged
}

/// What a result says, however the harness wrote it down: a result that came
/// back as text blocks says the same thing a plain-text one does, so it reads
/// the same way, both when it is set and when it is weighed as an
/// acknowledgement. Blocks that aren't all text are content in their own right,
/// and come back as themselves.
pub fn spoken(content: &ToolResultContent) -> Result<Cow<'_, str>, &[Block]> {
    match content {
        ToolResultContent::Text(text) => Ok(Cow::Borrowed(text)),
        ToolResultContent::Blocks(blocks) => spoken_blocks(blocks).map(Cow::Owned).ok_or(blocks),
    }
}

/// What a result made only of text blocks says, or `None` when it carries
/// anything else (an image, a reference) that is content in its own right.
fn spoken_blocks(blocks: &[Block]) -> Option<String> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Known(Known::Text { text }) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Option<Vec<&str>>>()
        .filter(|spoken| !spoken.is_empty())
        .map(|spoken| spoken.join("\n"))
}

/// A result that came back as blocks rather than text. A tool search answers
/// with references to the tools it found, which are names; everything else is
/// content in its own right and renders as the block it is.
fn blocks_result(scribe: &Scribe, tool: Option<&str>, blocks: &[Block]) -> Markup {
    let names: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Unknown(value) => text_of(value, "tool_name"),
            Block::Known(_) => None,
        })
        .collect();
    if tool == Some("ToolSearch") && names.len() == blocks.len() && !names.is_empty() {
        return html! {
            ul .tool.tool--references {
                @for name in names { li .tool__reference { (name) } }
            }
        };
    }
    html! { @for block in blocks { (scribe.block(block, false)) } }
}

/// The language token for a path, taken from its extension. syntect resolves it
/// by extension or name, so the bare extension is enough; an empty token (no
/// extension) highlights as plain text.
fn lang_for_path(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
}

/// An `Edit`'s before/after rendered as a diff body: the old lines removed, the
/// new lines added. The `diff` lexer colours each line by its leading marker,
/// reusing the inserted/deleted scope palette the stylesheet already defines.
fn unified_diff(old: &str, new: &str) -> String {
    let mut diff = String::new();
    for line in old.lines() {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// The leading `# ` heading of a markdown document, which titles it.
fn heading(document: &str) -> Option<&str> {
    document
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
}

/// A timeout in the units a reader thinks in, from the milliseconds the harness
/// records.
fn duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3_600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_names_the_lines_it_asked_for() {
        assert_eq!(span(Some(105), Some(4)).as_deref(), Some("lines 105–108"));
        assert_eq!(span(Some(105), None).as_deref(), Some("from line 105"));
        assert_eq!(span(None, Some(40)).as_deref(), Some("first 40 lines"));
        assert_eq!(span(None, None), None);
    }

    #[test]
    fn a_read_that_asked_for_no_span_names_none() {
        // Numbers off the wire, so a limit of zero and one that runs past the
        // end of the counting are both shapes a transcript can carry. Neither
        // names lines, and neither is worth failing a render over.
        assert_eq!(span(Some(105), Some(0)), None);
        assert_eq!(span(None, Some(0)), None);
        assert_eq!(span(Some(u64::MAX), Some(4)), None);
    }

    #[test]
    fn a_listing_loses_the_harness_numbering() {
        let listing = "     1\tuse std::fs;\n     2\t\n     3\tfn main() {}";

        assert_eq!(unnumbered(listing), "use std::fs;\n\nfn main() {}");
    }

    #[test]
    fn a_file_whose_own_lines_start_with_numbers_keeps_them() {
        // Nothing here is a listing prefix, so the text stands as it is rather
        // than losing a column of real content.
        let csv = "1\t2\t3\nyear\tcount";

        assert_eq!(unnumbered(csv), csv);
    }

    #[test]
    fn timeouts_read_in_the_units_they_were_set_in() {
        assert_eq!(duration(45_000), "45s");
        assert_eq!(duration(600_000), "10m");
        assert_eq!(duration(7_200_000), "2h");
    }

    #[test]
    fn a_workflow_is_named_by_the_meta_block_when_nothing_else_names_it() {
        let script = "export const meta = {\n  name: 'centre-the-bestiary',\n}\n";

        assert_eq!(
            workflow_name(script).as_deref(),
            Some("centre-the-bestiary")
        );
        assert_eq!(workflow_name("const x = 1"), None);
    }

    #[test]
    fn a_task_answer_parses_into_its_tagged_facts() {
        let answer = "<status>completed</status>\n\n<output>\nAll done.\n</output>";

        assert_eq!(
            tags(answer),
            [("status", "completed"), ("output", "All done.")]
        );
    }

    #[test]
    fn a_result_saying_only_that_the_call_worked_is_an_acknowledgement() {
        assert!(acknowledges(
            "Edit",
            "The file /src/render.rs has been updated successfully. \
             (file state is current in your context — no need to Read it back)"
        ));
        assert!(acknowledges(
            "Write",
            "File created successfully at: /src/tools.rs"
        ));
        assert!(acknowledges(
            "TaskUpdate",
            "Updated task #3 subject, status"
        ));
    }

    #[test]
    fn a_result_carrying_more_than_the_acknowledgement_is_not_one() {
        // The edit worked, but the file moved underneath it: that warning is
        // the only place a reader learns of it.
        assert!(!acknowledges(
            "Edit",
            "The file /src/render.rs has been updated successfully. \
             (note: the file had been modified on disk since you last read it)"
        ));
        // The same sentence from another tool is not an acknowledgement of a
        // call this one never made.
        assert!(!acknowledges("Bash", "Updated task #3 status"));
    }

    fn inks(text: &str) -> Vec<(Option<String>, &str)> {
        ansi_runs(text)
            .into_iter()
            .map(|(ink, run)| (ink.classes(), run))
            .collect()
    }

    #[test]
    fn colour_holds_until_the_terminal_changes_it() {
        assert_eq!(
            inks("plain \u{1b}[32mgreen \u{1b}[1mand bold\u{1b}[0m back"),
            [
                (None, "plain "),
                (Some("ansi ansi--green".to_owned()), "green "),
                (Some("ansi ansi--green ansi--bold".to_owned()), "and bold"),
                (None, " back"),
            ]
        );
    }

    #[test]
    fn a_sequence_that_drives_the_terminal_leaves_nothing_behind() {
        // Erase-line and cursor-home say nothing about the text around them,
        // and neither does a lone escape.
        assert_eq!(
            inks("\u{1b}[2Kone\u{1b}[Htwo\u{1b}three"),
            [(None, "one"), (None, "two"), (None, "three")]
        );
    }

    #[test]
    fn the_sixteen_named_colours_stay_the_folios_to_grind() {
        // A palette token can stand for these, so they resolve to a class and
        // carry no colour of their own.
        assert_eq!(indexed(1), Pigment::Named("red"));
        assert_eq!(indexed(9), Pigment::Named("bright-red"));
        assert_eq!(Pigment::Named("red").hex(), None);
    }

    #[test]
    fn a_colour_stated_outright_is_carried_as_its_own_value() {
        let mut ink = Ink::default();

        // The 6x6x6 cube, the grey ramp, and a 24-bit colour: none of these is
        // a colour the folio has a name for.
        ink.apply("38;5;208");
        assert_eq!(ink.style().as_deref(), Some("color:#ff8700"));
        ink.apply("38;5;244");
        assert_eq!(ink.style().as_deref(), Some("color:#808080"));
        ink.apply("48;2;120;90;200");
        assert_eq!(
            ink.style().as_deref(),
            Some("color:#808080;background-color:#785ac8")
        );
    }

    #[test]
    fn an_extended_colour_does_not_swallow_the_codes_after_it() {
        let mut ink = Ink::default();

        ink.apply("38;5;208;1;4");

        assert_eq!(ink.style().as_deref(), Some("color:#ff8700"));
        assert_eq!(
            ink.classes().as_deref(),
            Some("ansi ansi--bold ansi--underline")
        );
    }

    #[test]
    fn a_plan_is_titled_by_its_leading_heading() {
        assert_eq!(
            heading("# Centre the bestiary\n\nBody."),
            Some("Centre the bestiary")
        );
        assert_eq!(heading("No heading here."), None);
    }
}
