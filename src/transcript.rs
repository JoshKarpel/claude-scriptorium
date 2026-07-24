//! Parsing of Claude Code session JSONL into typed conversation values.

use std::{
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
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: Entry = serde_json::from_str(line)
                .with_context(|| format!("{}:{}", source.display(), index + 1))?;
            match entry {
                Entry::User(turn) => turns.push(turn.into_turn(Role::User)),
                Entry::Assistant(turn) => turns.push(turn.into_turn(Role::Assistant)),
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

/// A conversation turn, with the role lifted out of the JSONL's `type` tag.
#[derive(Debug)]
pub struct Turn {
    pub role: Role,
    pub timestamp: Timestamp,
    pub model: Option<String>,
    pub content: Content,
    /// True for turns belonging to a subagent running inside this session.
    pub is_sidechain: bool,
    /// True for turns the harness injected rather than the user typing them.
    pub is_meta: bool,
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
    pub blocks: Vec<Block>,
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
            blocks: turn.blocks(),
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
    /// Lines that carry no conversation: attachments, hook output, mode
    /// changes, file-history snapshots, and whatever else gets added later.
    #[serde(other)]
    Bookkeeping,
}

#[derive(Debug, Deserialize)]
struct RawTurn {
    timestamp: Timestamp,
    message: Message,
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
            content: self.message.content,
            is_sidechain: self.is_sidechain,
            is_meta: self.is_meta,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Content,
    model: Option<String>,
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

    fn is_visible_text(&self) -> bool {
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
