//! Parsing of Claude Code session JSONL into typed conversation values.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;

/// One rendered session: the unit the tool turns into a single HTML file.
#[derive(Debug)]
pub struct Folio {
    pub source: PathBuf,
    pub turns: Vec<Turn>,
}

impl Folio {
    /// Reads a session JSONL file, keeping only the lines that carry
    /// conversation.
    pub fn read(source: &Path) -> Result<Self> {
        let text = fs::read_to_string(source)
            .with_context(|| format!("reading session {}", source.display()))?;

        let mut turns = Vec::new();
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
                Entry::User(turn) => turns.push(turn.into_turn(Role::User)),
                Entry::Assistant(raw) => {
                    let opens_response = raw
                        .message
                        .id
                        .as_deref()
                        .is_none_or(|id| counted.insert(id.to_owned()));
                    let turn = raw.into_turn(Role::Assistant);
                    turns.push(Turn {
                        usage: turn.usage.filter(|_| opens_response),
                        ..turn
                    });
                }
                Entry::Attachment(attachment) => turns.extend(attachment.into_turn()),
                Entry::Bookkeeping => {}
            }
        }

        Ok(Self {
            source: source.to_path_buf(),
            turns,
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

    /// What the whole session cost, or `None` when no turn reports usage. The
    /// cached prefix counts once per turn that read it, since each turn reads
    /// it again.
    pub fn usage(&self) -> Option<Usage> {
        self.turns
            .iter()
            .filter_map(|turn| turn.usage)
            .reduce(|total, usage| total + usage)
    }

    /// Folds the raw turns into the display stream: drops `/clear` boundaries
    /// and merges each tool-result turn back into the assistant turn it
    /// answers, so a call and its result render as one panel.
    pub fn panels(&self) -> Vec<Panel> {
        let mut panels: Vec<Panel> = Vec::new();
        for (index, turn) in self.turns.iter().enumerate() {
            if turn.is_clear_command() {
                continue;
            }
            if turn.is_tool_response()
                && let Some(assistant) = panels.last_mut().filter(|p| p.role == Role::Assistant)
            {
                assistant.blocks.extend(turn.blocks());
                continue;
            }
            panels.push(Panel::from_turn(turn, index + 1));
        }
        panels
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
    /// Everything the model read: fresh input plus the cached prefix, whether
    /// that prefix was written this turn or replayed from an earlier one.
    pub fn context(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }
}

impl std::ops::Add for Usage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens
                + other.cache_creation_input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens + other.cache_read_input_tokens,
        }
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

    /// The content as a uniform block list, lifting a plain string into a
    /// single text block so every panel is a sequence of blocks.
    fn blocks(&self) -> Vec<Block> {
        match &self.content {
            Content::Text(text) => vec![Block::Known(Known::Text { text: text.clone() })],
            Content::Blocks(blocks) => blocks.clone(),
        }
    }
}

/// One speaker's contribution as displayed. Folding the wire-level turns into
/// panels is where harness scaffolding is filtered and tool results are
/// reunited with the assistant that called them, so the renderer walks an
/// already-clean stream and never re-derives any of it.
#[derive(Debug)]
pub struct Panel {
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
    /// True for panels the harness injected rather than the user typing them.
    pub is_meta: bool,
}

impl Panel {
    fn from_turn(turn: &Turn, turn_number: usize) -> Self {
        Self {
            turn_number,
            role: turn.role,
            timestamp: turn.timestamp,
            model: turn.model.clone(),
            effort: turn.effort.clone(),
            blocks: turn.blocks(),
            usage: turn.usage,
            is_sidechain: turn.is_sidechain,
            is_meta: turn.is_meta,
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
    /// Attachment lines. Most are scaffolding (hook output, task reminders,
    /// memory), but a `queued_command` carries a message the user typed while
    /// the assistant was still working: real conversation the harness records
    /// here rather than as a `user` turn, so it must not be dropped.
    #[serde(rename = "attachment")]
    Attachment(RawAttachment),
    /// Lines that carry no conversation: hook output, mode changes,
    /// file-history snapshots, and whatever else gets added later.
    #[serde(other)]
    Bookkeeping,
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
        }
    }
}

/// An `attachment` line. Only a `queued_command` body becomes a turn; every
/// other attachment kind is scaffolding this drops.
#[derive(Debug, Deserialize)]
struct RawAttachment {
    timestamp: Timestamp,
    attachment: AttachmentBody,
}

impl RawAttachment {
    /// A turn for a message the user queued mid-response, or `None` for any
    /// other attachment kind.
    fn into_turn(self) -> Option<Turn> {
        let AttachmentBody::QueuedCommand { prompt } = self.attachment else {
            return None;
        };
        Some(Turn {
            role: Role::User,
            timestamp: self.timestamp,
            model: None,
            effort: None,
            content: Content::Text(prompt),
            usage: None,
            is_sidechain: false,
            is_meta: false,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AttachmentBody {
    /// A message the user typed while the assistant was still working, dequeued
    /// and processed later in the same session.
    QueuedCommand { prompt: String },
    /// Every other attachment kind: hook output, task reminders, memory, and
    /// whatever else gets added later, none of it conversation.
    #[serde(other)]
    Other,
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
    fn is_only_tool_results(&self) -> bool {
        match self {
            Content::Text(_) => false,
            Content::Blocks(blocks) => {
                !blocks.is_empty() && blocks.iter().all(Block::is_tool_result)
            }
        }
    }
}

impl Block {
    fn is_tool_result(&self) -> bool {
        matches!(self, Block::Known(Known::ToolResult { .. }))
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
/// role already has a colour, so the label names the content instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    User,
    Assistant,
    Tool,
    Thinking,
}

impl PanelKind {
    pub fn label(self) -> &'static str {
        match self {
            PanelKind::User => "user",
            PanelKind::Assistant => "assistant",
            PanelKind::Tool => "tool",
            PanelKind::Thinking => "thinking",
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
        name: String,
        input: Value,
    },
    ToolResult {
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
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
