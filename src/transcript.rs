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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

/// A content block, or the raw JSON of one this version doesn't recognize.
///
/// Claude Code's transcript format grows new block types; encountering one is
/// a producer adding something optional, not malformed input, so it renders as
/// JSON instead of aborting the folio.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Block {
    Known(Known),
    Unknown(Value),
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Debug, Deserialize)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}
