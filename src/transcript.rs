//! Parsing of Claude Code session JSONL into typed conversation values.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    gloss::{self, Gloss, GlossKind, Wrapped},
    tools,
};

/// One line of a session as the folio keeps it: a turn of the conversation, or
/// a note the harness wrote into the session around it.
#[derive(Debug)]
pub enum Recorded {
    Turn(Turn),
    Gloss(Glossed),
}

/// A harness note where it sits in the session.
#[derive(Debug)]
pub struct Glossed {
    pub timestamp: Timestamp,
    pub is_sidechain: bool,
    pub gloss: Gloss,
    /// The session's own name for this line.
    pub uuid: Option<String>,
    /// The line this note was written about, where it names one. A slash
    /// command's output is a `system` line naming the command's own line as its
    /// parent, which is the exact key that gathers the two into one panel.
    pub answers: Option<String>,
}

/// One rendered session: the unit the tool turns into a single HTML file.
#[derive(Debug)]
pub struct Folio {
    pub source: PathBuf,
    /// The session as read, in file order, so a harness note keeps its place
    /// among the turns it stands between.
    pub recorded: Vec<Recorded>,
}

impl Folio {
    /// Reads a session JSONL file, keeping the lines that carry conversation
    /// and the notes the harness wrote around them.
    pub fn read(source: &Path) -> Result<Self> {
        let text = fs::read_to_string(source)
            .with_context(|| format!("reading session {}", source.display()))?;

        let mut recorded = Vec::new();
        // One API response is written as several lines, one per content block,
        // each repeating the response's usage. Counting every line would
        // multiply what the response cost, so a response is counted once, on
        // the first line that carries its id.
        let mut counted = HashSet::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: Entry = serde_json::from_str(line)
                .with_context(|| format!("{}:{}", source.display(), index + 1))?;
            match entry {
                Entry::User(turn) => recorded.push(Recorded::Turn(turn.into_turn(Role::User))),
                Entry::Assistant(raw) => {
                    let opens_response = raw
                        .message
                        .id
                        .as_deref()
                        .is_none_or(|id| counted.insert(id.to_owned()));
                    let turn = raw.into_turn(Role::Assistant);
                    recorded.push(Recorded::Turn(Turn {
                        usage: turn.usage.filter(|_| opens_response),
                        ..turn
                    }));
                }
                Entry::Attachment(attachment) => recorded.extend(attachment.into_recorded()),
                Entry::System(line) => recorded.extend(glossed_system(&line)),
                Entry::Bookkeeping => {}
            }
        }

        Ok(Self {
            source: source.to_path_buf(),
            recorded,
        })
    }

    /// The conversation alone, for the figures that are about what the model
    /// was sent and what it wrote.
    pub fn turns(&self) -> impl Iterator<Item = &Turn> {
        self.recorded.iter().filter_map(|recorded| match recorded {
            Recorded::Turn(turn) => Some(turn),
            Recorded::Gloss(_) => None,
        })
    }

    pub fn session_id(&self) -> &str {
        self.source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("session")
    }

    /// Cheaply scans a session's listing metadata (its title and working
    /// directory) without parsing the conversation, tolerating malformed lines
    /// so one bad session never breaks a picker that lists every session. This
    /// is deliberately lenient where [`Folio::read`] is strict: a label is
    /// best-effort, a render is not.
    pub fn peek(source: &Path) -> SessionPeek {
        let Ok(text) = fs::read_to_string(source) else {
            return SessionPeek::default();
        };

        let mut cwd = None;
        let mut ai_title = None;
        let mut first_prompt = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if cwd.is_none()
                && let Some(dir) = value.get("cwd").and_then(Value::as_str)
            {
                cwd = Some(PathBuf::from(dir));
            }
            match value.get("type").and_then(Value::as_str) {
                // Claude rewrites the title as the session evolves, so the last
                // one wins: it is the summary Claude Code shows in terminals.
                Some("ai-title") => {
                    if let Some(title) = value.get("aiTitle").and_then(Value::as_str) {
                        ai_title = Some(title.to_owned());
                    }
                }
                Some("user") if first_prompt.is_none() && !is_meta(&value) => {
                    first_prompt = user_prompt(&value);
                }
                _ => {}
            }
        }

        SessionPeek {
            cwd,
            title: ai_title.or(first_prompt),
        }
    }

    /// The output across the session, or `None` when no turn reports usage.
    /// Output totals, since each turn produces its own.
    pub fn output(&self) -> Option<u64> {
        self.turns()
            .filter_map(|turn| turn.usage)
            .map(|usage| usage.output_tokens)
            .reduce(|total, output| total + output)
    }

    /// The largest input any one turn took, or `None` when no turn reports
    /// usage: how big the conversation ever got. A high-water mark rather than
    /// a sum, since every turn is sent the whole conversation and summing that
    /// would count the same text once per turn that saw it.
    pub fn largest_input(&self) -> Option<u64> {
        self.turns()
            .filter_map(|turn| turn.usage)
            .map(|usage| usage.input())
            .max()
    }

    /// Folds the raw stream into the display one: drops `/clear` boundaries,
    /// merges each tool-result turn back into the assistant turn it answers so
    /// a call and its result render as one panel, and lifts the turns the
    /// harness wrote for itself out of the conversation and into glosses.
    pub fn panels(&self) -> Vec<Panel> {
        let calls = self.calls();
        let mut panels: Vec<Panel> = Vec::new();
        // Where each call ended up, so the result answering it can be put in
        // the same panel however many panels later it comes back.
        let mut homes: HashMap<&str, usize> = HashMap::new();
        // The lines the folio left unset, so a note written *about* one goes the
        // same way. A slash command that only works the harness is dropped, and
        // what it printed is recorded as a line of its own: keeping that would
        // set the output of a command the folio deliberately never mentions.
        let mut unset: HashSet<&str> = HashSet::new();
        for (index, recorded) in self.recorded.iter().enumerate() {
            let turn_number = index + 1;
            let turn = match recorded {
                Recorded::Gloss(glossed) => {
                    if glossed
                        .answers
                        .as_deref()
                        .is_some_and(|parent| unset.contains(parent))
                    {
                        continue;
                    }
                    // One hook writes several lines (what it decided, what it
                    // injected, what it printed) and a slash command's output is
                    // written as a line naming the command's own. Either way the
                    // note belongs in the panel already open, the way a tool
                    // result belongs in the panel holding its call.
                    if let Some(open) = panels
                        .last_mut()
                        .and_then(Panel::as_gloss_mut)
                        .filter(|panel| panel.gathers(glossed))
                    {
                        open.gloss.absorb(glossed.gloss.clone());
                        continue;
                    }
                    panels.push(Panel::Gloss(GlossPanel::of(glossed, turn_number)));
                    continue;
                }
                Recorded::Turn(turn) => turn,
            };
            if turn.is_clear_command() {
                // A boundary is dropped like any other command the folio leaves
                // unset, so what it printed goes with it rather than orphaning
                // into a panel of its own with nothing to say which command it
                // came from.
                unset.extend(turn.uuid.as_deref());
                continue;
            }
            match turn.wrapped() {
                Some(Wrapped::Note(mut gloss)) => {
                    // A skill names the directory it was loaded from, which is
                    // how `gloss::meta` knows one. A built-in has no directory
                    // on disk, so its instructions arrive as bare prose and read
                    // as a passing note; the command standing directly in front
                    // of them is what says what they are. Relabelling here is
                    // what gives a skill one shape however it was loaded, since
                    // the model reaches for one with no command at all.
                    if gloss.kind == GlossKind::Note
                        && let Some(command) = panels
                            .last()
                            .and_then(Panel::as_gloss)
                            .filter(|panel| panel.gloss.kind == GlossKind::Command)
                            .and_then(|panel| panel.gloss.gist.as_deref())
                    {
                        gloss.ran_by(command);
                    }
                    panels.push(Panel::Gloss(GlossPanel {
                        turn_number,
                        timestamp: turn.timestamp,
                        is_sidechain: turn.is_sidechain,
                        gloss,
                        uuid: turn.uuid.clone(),
                    }));
                    continue;
                }
                Some(Wrapped::Nothing) => {
                    unset.extend(turn.uuid.as_deref());
                    continue;
                }
                None => {}
            }
            let blocks = answered(turn.blocks(), &calls);
            // A turn whose every block was dropped has nothing left to show,
            // and an empty panel is a bordered box with no contents in it.
            if blocks.is_empty() {
                continue;
            }
            if turn.is_tool_response() {
                // Each result joins the panel holding the call it answers, not
                // whichever panel is newest. Calls issued together are written
                // as one assistant line each, so they are several panels, and
                // taking the last one piles every result onto the last call
                // while its siblings show none.
                let mut homeless = Vec::new();
                for block in blocks {
                    let home = block.answering().and_then(|id| homes.get(id)).copied();
                    match home
                        .and_then(|at| panels.get_mut(at))
                        .and_then(Panel::answering_assistant)
                    {
                        Some(speech) => speech.blocks.push(block),
                        None => homeless.push(block),
                    }
                }
                if homeless.is_empty() {
                    continue;
                }
                // A result whose call this session never recorded still belongs
                // with the assistant that was speaking.
                if let Some(speech) = panels.last_mut().and_then(Panel::answering_assistant) {
                    speech.blocks.extend(homeless);
                    continue;
                }
                panels.push(Panel::Speech(Speech::of(turn, turn_number, homeless)));
                continue;
            }
            for calling in turn.calling() {
                homes.insert(calling, panels.len());
            }
            panels.push(Panel::Speech(Speech::of(turn, turn_number, blocks)));
        }
        panels
    }

    /// Every tool call in the session, by id. The wire format names the tool
    /// only on the call: a result carries just the id it answers, so a result
    /// can only be set the way its call is once the two are matched up.
    fn calls(&self) -> HashMap<&str, Answered> {
        self.turns()
            .flat_map(|turn| match &turn.content {
                Content::Text(_) => [].iter(),
                Content::Blocks(blocks) => blocks.iter(),
            })
            .filter_map(|block| match block {
                Block::Known(Known::ToolUse {
                    id: Some(id),
                    name,
                    input,
                }) => Some((id.as_str(), Answered::of(name, input))),
                _ => None,
            })
            .collect()
    }
}

/// Names each result in `blocks` with the call it answers, and drops the ones
/// that say nothing their call doesn't. Naming has to come first: whether a
/// result is worth showing is a question about the tool that produced it.
fn answered(mut blocks: Vec<Block>, calls: &HashMap<&str, Answered>) -> Vec<Block> {
    for block in &mut blocks {
        if let Block::Known(Known::ToolResult {
            tool_use_id: Some(id),
            answers,
            ..
        }) = block
        {
            *answers = calls.get(id.as_str()).cloned();
        }
    }
    blocks.retain(|block| !is_acknowledgement(block));
    blocks
}

/// True for a result that only confirms its call was carried out. A failure is
/// never one of these: that a call *didn't* work is the whole of what it says.
fn is_acknowledgement(block: &Block) -> bool {
    let Block::Known(Known::ToolResult {
        content,
        is_error: false,
        answers: Some(answered),
        ..
    }) = block
    else {
        return false;
    };
    tools::spoken(content).is_ok_and(|text| tools::acknowledges(&answered.tool, &text))
}

/// What a result needs to know about the call it answers: which tool ran, and
/// the path it ran on where the tool has one, since a file's contents are set
/// by its extension and only the call records the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub tool: String,
    pub subject: Option<String>,
}

impl Answered {
    fn of(tool: &str, input: &Value) -> Self {
        Self {
            tool: tool.to_owned(),
            subject: input
                .get("file_path")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }
}

/// A session's listing metadata, scanned by [`Folio::peek`] for pickers and
/// indexes that show sessions without rendering them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SessionPeek {
    /// The directory the session ran in, recovered from the transcript because
    /// the encoded project-dir name flattens separators and can't be decoded
    /// back to a real path.
    pub cwd: Option<PathBuf>,
    /// A human label for the session: Claude's own `ai-title`, falling back to
    /// the first prose the user typed when the session has no title yet.
    pub title: Option<String>,
}

/// True when a turn was injected by the harness rather than typed by the user.
fn is_meta(entry: &Value) -> bool {
    entry
        .get("isMeta")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The first prose from a user turn, or `None` when it carries only a
/// harness-injected command wrapper, notification, or reminder: those open with
/// an XML-ish tag rather than something the user actually wrote.
fn user_prompt(entry: &Value) -> Option<String> {
    const WRAPPERS: [&str; 4] = [
        "<command-",
        "<local-command-",
        "<task-notification>",
        "<system-reminder>",
    ];

    let content = entry.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.trim(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .find_map(|block| block.get("text").and_then(Value::as_str))?
            .trim(),
        _ => return None,
    };

    let is_wrapper = WRAPPERS.iter().any(|tag| text.starts_with(tag));
    (!text.is_empty() && !is_wrapper).then(|| text.to_owned())
}

/// A conversation turn, with the role lifted out of the JSONL's `type` tag.
#[derive(Debug)]
pub struct Turn {
    pub role: Role,
    pub timestamp: Timestamp,
    pub model: Option<String>,
    /// How hard the model was asked to think, where the harness records it: a
    /// refinement of the model, not a separate fact about the turn.
    pub effort: Option<String>,
    pub content: Content,
    /// What the turn cost, for the assistant turns that report it. A user turn
    /// has none, and transcripts written before the harness recorded usage
    /// carry none either.
    pub usage: Option<Usage>,
    /// True for turns belonging to a subagent running inside this session.
    pub is_sidechain: bool,
    /// True for turns the harness injected rather than the user typing them.
    pub is_meta: bool,
    /// The session's own name for this line, which is what a note written about
    /// it names as its parent. A slash command's output is recorded as its own
    /// `system` line pointing back here, so this is what lets the two be set as
    /// one panel.
    pub uuid: Option<String>,
}

/// What one turn cost: what the model read, split by where it came from, and
/// what it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl Usage {
    /// Everything the model was sent for this turn, which is the conversation
    /// as it stood. The transcript splits it by where it was served from
    /// (fresh, cached this turn, replayed from an earlier one), a billing
    /// distinction rather than anything about the conversation, so this
    /// recombines it.
    pub fn input(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }

    /// The part of that input the model had not been sent before: what this
    /// turn added to the conversation, which is what the turn's own output can
    /// be read against. The cache holds exactly the prefix already sent, so its
    /// boundary marks what is new, except where a lapsed cache re-sends a
    /// prefix and counts it new again.
    pub fn uncached_input(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens
    }
}

impl Turn {
    /// True when a `user`-role turn carries only tool results: the harness
    /// returning tool output, not the user typing. These read as a
    /// continuation of the assistant's turn, not a message of their own.
    pub fn is_tool_response(&self) -> bool {
        self.role == Role::User && self.content.is_only_tool_results()
    }

    /// True when this turn is the `/clear` slash command, which resets the
    /// context: a session boundary the harness records as a user turn, with
    /// no conversation of its own worth showing.
    pub fn is_clear_command(&self) -> bool {
        matches!(&self.content, Content::Text(text)
            if text.contains("<command-name>/clear</command-name>"))
    }

    /// What this turn really is, for the turns in the user's role that are not
    /// the user: a note the harness wrote for itself (a skill's instructions),
    /// a slash command the user ran, or a wrapper standing in front of one.
    /// All three are recorded in the user's role because that is where they
    /// enter the conversation, and none of them is the user speaking.
    fn wrapped(&self) -> Option<Wrapped> {
        if self.role != Role::User {
            return None;
        }
        let said = self.content.spoken()?;
        gloss::wrapped(&said, self.is_meta)
    }

    /// The ids of the calls this turn issues, so a result coming back later can
    /// be put in the panel that holds its call.
    fn calling(&self) -> impl Iterator<Item = &str> {
        match &self.content {
            Content::Text(_) => [].iter(),
            Content::Blocks(blocks) => blocks.iter(),
        }
        .filter_map(|block| match block {
            Block::Known(Known::ToolUse { id: Some(id), .. }) => Some(id.as_str()),
            _ => None,
        })
    }

    /// The content as a uniform block list, lifting a plain string into a
    /// single text block so every panel is a sequence of blocks.
    fn blocks(&self) -> Vec<Block> {
        match &self.content {
            Content::Text(text) => vec![Block::Known(Known::Text { text: text.clone() })],
            Content::Blocks(blocks) => blocks.clone(),
        }
    }
}

/// One article of the folio: a speaker's contribution, or a note the harness
/// wrote into the session. Folding the wire-level stream into panels is where
/// scaffolding is filtered, tool results are reunited with the assistant that
/// called them, and injected context is lifted out of the conversation, so the
/// renderer walks an already-clean stream and never re-derives any of it.
#[derive(Debug)]
pub enum Panel {
    Speech(Speech),
    Gloss(GlossPanel),
}

impl Panel {
    /// The assistant speech this panel is, for a tool result looking for the
    /// call it answers. A gloss never absorbs one: a hook firing between a call
    /// and its result does not make the result the hook's.
    fn answering_assistant(&mut self) -> Option<&mut Speech> {
        match self {
            Panel::Speech(speech) if speech.role == Role::Assistant => Some(speech),
            _ => None,
        }
    }

    fn as_gloss(&self) -> Option<&GlossPanel> {
        match self {
            Panel::Gloss(gloss) => Some(gloss),
            Panel::Speech(_) => None,
        }
    }

    fn as_gloss_mut(&mut self) -> Option<&mut GlossPanel> {
        match self {
            Panel::Gloss(gloss) => Some(gloss),
            Panel::Speech(_) => None,
        }
    }

    pub fn is_sidechain(&self) -> bool {
        match self {
            Panel::Speech(speech) => speech.is_sidechain,
            Panel::Gloss(gloss) => gloss.is_sidechain,
        }
    }

    pub fn kind(&self) -> PanelKind {
        match self {
            Panel::Speech(speech) => speech.kind(),
            Panel::Gloss(gloss) => PanelKind::Gloss(gloss.gloss.kind),
        }
    }

    pub fn timestamp(&self) -> Timestamp {
        match self {
            Panel::Speech(speech) => speech.timestamp,
            Panel::Gloss(gloss) => gloss.timestamp,
        }
    }

    pub fn turn_number(&self) -> usize {
        match self {
            Panel::Speech(speech) => speech.turn_number,
            Panel::Gloss(gloss) => gloss.turn_number,
        }
    }
}

/// One speaker's contribution as displayed.
#[derive(Debug)]
pub struct Speech {
    /// The 1-based position of this panel's leading turn in the raw stream.
    /// Gaps between successive panels mark turns that were folded in or
    /// dropped, so a panel that spans several turns still has one stable label.
    pub turn_number: usize,
    pub role: Role,
    pub timestamp: Timestamp,
    pub model: Option<String>,
    /// How hard the model was asked to think, where the harness records it.
    pub effort: Option<String>,
    pub blocks: Vec<Block>,
    /// What the panel's leading turn cost. The tool-result turns folded in
    /// carry none of their own: the assistant turn that called the tool is
    /// where the harness records what the exchange cost.
    pub usage: Option<Usage>,
    /// True for panels belonging to a subagent running inside this session.
    pub is_sidechain: bool,
}

impl Speech {
    fn of(turn: &Turn, turn_number: usize, blocks: Vec<Block>) -> Self {
        Self {
            turn_number,
            role: turn.role,
            timestamp: turn.timestamp,
            model: turn.model.clone(),
            effort: turn.effort.clone(),
            blocks,
            usage: turn.usage,
            is_sidechain: turn.is_sidechain,
        }
    }

    /// The panel's content kind, preferring the most user-facing thing it
    /// carries: visible prose reads as the speaker, otherwise a tool exchange,
    /// otherwise bare reasoning.
    pub fn kind(&self) -> PanelKind {
        let speaker = match self.role {
            Role::User => PanelKind::User,
            Role::Assistant => PanelKind::Assistant,
        };
        if self.blocks.iter().any(Block::is_visible_text) {
            speaker
        } else if self.blocks.iter().any(Block::is_tool) {
            PanelKind::Tool
        } else if self.blocks.iter().any(Block::is_thinking) {
            PanelKind::Thinking
        } else {
            speaker
        }
    }
}

/// One harness note as displayed.
#[derive(Debug)]
pub struct GlossPanel {
    pub turn_number: usize,
    pub timestamp: Timestamp,
    pub is_sidechain: bool,
    pub gloss: Gloss,
    /// What this panel's leading line was called, so a note written about that
    /// line can be gathered into it.
    uuid: Option<String>,
}

impl GlossPanel {
    fn of(glossed: &Glossed, turn_number: usize) -> Self {
        Self {
            turn_number,
            timestamp: glossed.timestamp,
            is_sidechain: glossed.is_sidechain,
            gloss: glossed.gloss.clone(),
            uuid: glossed.uuid.clone(),
        }
    }

    /// True when the note belongs in this panel rather than one of its own: one
    /// firing of a hook writes several lines, and a slash command's output is
    /// written as a line naming the command's own.
    fn gathers(&self, glossed: &Glossed) -> bool {
        if self.gloss.same_firing_as(&glossed.gloss) {
            return true;
        }
        matches!((&self.uuid, &glossed.answers), (Some(mine), Some(theirs)) if mine == theirs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One line of a session JSONL file.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Entry {
    #[serde(rename = "user")]
    User(RawTurn),
    #[serde(rename = "assistant")]
    Assistant(RawTurn),
    /// Attachment lines: a message the user queued mid-response, the notes the
    /// harness wrote into the session (hook output, the memory files it pulled
    /// in, plan-mode boundaries), and the scaffolding [`gloss::attachment`]
    /// leaves unset.
    #[serde(rename = "attachment")]
    Attachment(RawAttachment),
    /// What the harness said in its own voice. Most of it is bookkeeping it
    /// keeps for itself; [`gloss::system`] picks out the rest.
    #[serde(rename = "system")]
    System(Value),
    /// Lines that carry no conversation: mode changes, file-history snapshots,
    /// and whatever else gets added later.
    #[serde(other)]
    Bookkeeping,
}

/// The note a `system` line carries, where it carries one and records when it
/// was said. A line with no timestamp has no place in the stream to sit at.
fn glossed_system(line: &Value) -> Option<Recorded> {
    let timestamp = line.get("timestamp")?.as_str()?.parse().ok()?;
    let named = |field| line.get(field).and_then(Value::as_str).map(str::to_owned);
    Some(Recorded::Gloss(Glossed {
        timestamp,
        is_sidechain: line
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        gloss: gloss::system(line)?,
        uuid: named("uuid"),
        answers: named("parentUuid"),
    }))
}

#[derive(Debug, Deserialize)]
struct RawTurn {
    timestamp: Timestamp,
    message: Message,
    /// Recorded beside the message rather than in it, and only by harness
    /// versions that track it.
    effort: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default, rename = "isMeta")]
    is_meta: bool,
    #[serde(default)]
    uuid: Option<String>,
}

impl RawTurn {
    fn into_turn(self, role: Role) -> Turn {
        Turn {
            role,
            timestamp: self.timestamp,
            model: self.message.model,
            effort: self.effort,
            content: self.message.content,
            usage: self.message.usage,
            is_sidechain: self.is_sidechain,
            is_meta: self.is_meta,
            uuid: self.uuid,
        }
    }
}

/// An `attachment` line. The body stays raw JSON because its shapes are the
/// harness's rather than a contract: [`gloss::attachment`] reads each leniently
/// and leaves unset what it doesn't recognise, so an attachment that grows a
/// field can never abort a render.
#[derive(Debug, Deserialize)]
struct RawAttachment {
    timestamp: Timestamp,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default)]
    uuid: Option<String>,
    attachment: Value,
}

impl RawAttachment {
    /// What this attachment contributes to the session: a user turn for the
    /// message the user queued while the assistant was still working (real
    /// conversation the harness records here rather than as a `user` line), a
    /// gloss for the notes it wrote into the session, and nothing for the
    /// scaffolding.
    fn into_recorded(self) -> Option<Recorded> {
        if self.attachment.get("type").and_then(Value::as_str) == Some("queued_command")
            && let Some(prompt) = self.attachment.get("prompt").and_then(Value::as_str)
        {
            return Some(Recorded::Turn(Turn {
                role: Role::User,
                timestamp: self.timestamp,
                model: None,
                effort: None,
                content: Content::Text(prompt.to_owned()),
                usage: None,
                is_sidechain: self.is_sidechain,
                is_meta: false,
                uuid: self.uuid,
            }));
        }
        Some(Recorded::Gloss(Glossed {
            timestamp: self.timestamp,
            is_sidechain: self.is_sidechain,
            gloss: gloss::attachment(&self.attachment)?,
            uuid: self.uuid,
            // A hook's notes are gathered by the firing they name rather than
            // by the line they were written about.
            answers: None,
        }))
    }
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Content,
    model: Option<String>,
    usage: Option<Usage>,
    /// The API response this line belongs to. Several lines share one, since a
    /// response is written a block at a time.
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

impl Content {
    /// What this turn says in words, or `None` when it carries anything that
    /// isn't text. The harness writes a turn's text as a bare string or as text
    /// blocks depending on how it arrived, which is a fact about the recording
    /// rather than about what was said.
    fn spoken(&self) -> Option<Cow<'_, str>> {
        match self {
            Content::Text(text) => Some(Cow::Borrowed(text)),
            Content::Blocks(blocks) => blocks
                .iter()
                .map(|block| match block {
                    Block::Known(Known::Text { text }) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Option<Vec<&str>>>()
                .filter(|spoken| !spoken.is_empty())
                .map(|spoken| Cow::Owned(spoken.join("\n"))),
        }
    }

    fn is_only_tool_results(&self) -> bool {
        match self {
            Content::Text(_) => false,
            Content::Blocks(blocks) => !blocks.is_empty() && blocks.iter().all(Block::is_result),
        }
    }
}

impl Block {
    /// The two halves of a tool exchange, paired so a reader of the code (and a
    /// test) can weigh a panel's calls against the results that reached it.
    pub fn is_call(&self) -> bool {
        matches!(self, Block::Known(Known::ToolUse { .. }))
    }

    pub fn is_result(&self) -> bool {
        matches!(self, Block::Known(Known::ToolResult { .. }))
    }

    /// The call this block answers, for a result that names one.
    fn answering(&self) -> Option<&str> {
        match self {
            Block::Known(Known::ToolResult {
                tool_use_id: Some(id),
                ..
            }) => Some(id.as_str()),
            _ => None,
        }
    }

    fn is_tool(&self) -> bool {
        matches!(
            self,
            Block::Known(Known::ToolUse { .. } | Known::ToolResult { .. })
        )
    }

    fn is_thinking(&self) -> bool {
        matches!(self, Block::Known(Known::Thinking { .. }))
    }

    pub(crate) fn is_visible_text(&self) -> bool {
        matches!(self, Block::Known(Known::Text { text }) if !text.trim().is_empty())
    }
}

/// What a panel actually shows, so a label can say more than "assistant": the
/// role already has a colour, so the label names the content instead. A gloss
/// is labelled by what wrote it rather than by what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    User,
    Assistant,
    Tool,
    Thinking,
    Gloss(GlossKind),
}

/// Which side of the exchange a panel is on.
///
/// This is the folio's one organising axis, and it is declared here so nothing
/// else has to restate it: the stylesheet pitches a kind's pigment warm or cool
/// by it, the dock steps along it, and the label a reader sees is the same
/// classification the code holds. A new [`PanelKind`] answers this question
/// before it is given a colour or a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the model produced: what it said, its reasoning, the tools it
    /// reached for. Set in the warm hues.
    Model,
    /// What reached the model from outside it: what the user said, the commands
    /// they typed, the skills they wrote, the hooks that answer for them. Set in
    /// the cool hues.
    Entered,
    /// Neither, deliberately. A plan boundary marks a division in the text
    /// rather than anything said in it, and a rule or a passing note is ambient:
    /// colouring every kind would leave nothing quiet, and stepping to every
    /// kind would make the dock no faster than scrolling.
    Aside,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Side::Model => "model",
            Side::Entered => "entered",
            Side::Aside => "aside",
        }
    }
}

impl PanelKind {
    /// Every kind a panel can be, in the order the key reads them: down the cool
    /// column, then down the warm one. The key builds its chips from this rather
    /// than restating the list, so a new kind reaches it by being declared here
    /// rather than by being remembered in the markup as well.
    ///
    /// The two halves are five and five so the key is a clean grid, which is why
    /// `note` sits at the foot of the warm column despite being [`Side::Aside`].
    /// Nothing reads the *order* as a classification: every chip carries its own
    /// side, so the dock still steps past `note` and its neutral ink still keeps
    /// it from reading as the model's.
    pub const EVERY: [PanelKind; 10] = [
        PanelKind::User,
        PanelKind::Gloss(GlossKind::Command),
        PanelKind::Gloss(GlossKind::Skill),
        PanelKind::Gloss(GlossKind::Hook),
        PanelKind::Gloss(GlossKind::Rule),
        PanelKind::Assistant,
        PanelKind::Thinking,
        PanelKind::Tool,
        PanelKind::Gloss(GlossKind::Plan),
        PanelKind::Gloss(GlossKind::Note),
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelKind::User => "user",
            PanelKind::Assistant => "assistant",
            PanelKind::Tool => "tool",
            PanelKind::Thinking => "thinking",
            PanelKind::Gloss(kind) => kind.label(),
        }
    }

    /// Which side of the exchange this kind belongs to. Exhaustive on purpose:
    /// adding a kind will not compile until its side is decided.
    ///
    /// A rule is the user's own writing pulled into the conversation, so it is
    /// theirs however the harness fetched it. A plan boundary belongs to the
    /// model: the mode is the user's to ask for, but entering and leaving it is
    /// the model reporting on its own working, which is why it reads beside the
    /// reasoning rather than beside the asking.
    pub fn side(self) -> Side {
        match self {
            PanelKind::Assistant | PanelKind::Tool | PanelKind::Thinking => Side::Model,
            PanelKind::Gloss(GlossKind::Plan) => Side::Model,
            PanelKind::User => Side::Entered,
            PanelKind::Gloss(
                GlossKind::Command | GlossKind::Skill | GlossKind::Hook | GlossKind::Rule,
            ) => Side::Entered,
            PanelKind::Gloss(GlossKind::Note) => Side::Aside,
        }
    }
}

/// A content block, or the raw JSON of one this version doesn't recognize.
///
/// Claude Code's transcript format grows new block types; encountering one is
/// a producer adding something optional, not malformed input, so it renders as
/// JSON instead of aborting the folio.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Block {
    Known(Known),
    Unknown(Value),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Known {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        /// What the result answering this call points back to.
        id: Option<String>,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: Option<String>,
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
        /// The call this answers. Never on the wire: it is resolved when the
        /// result is folded into the panel, so the renderer walks a stream
        /// where every result already knows which tool produced it.
        #[serde(skip)]
        answers: Option<Answered>,
    },
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}
