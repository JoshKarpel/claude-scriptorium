# claude-scriptorium

Render Claude Code sessions as self-contained HTML.

Each session becomes one **folio**: a single `.html` file with its markup,
styles, and images inlined, so it can be mailed, gisted, or opened offline
without dragging an asset directory along.

## Install

```bash
cargo install claude-scriptorium
```

## Usage

Render the most recent session recorded for the current directory:

```bash
claude-scriptorium render
```

Render a specific session file, to a chosen path:

```bash
claude-scriptorium render ~/.claude/projects/-home-me-work/<session-id>.jsonl -o folio.html
```

Serve a session over HTTP with live reload, for watching a session or iterating
on the rendering:

```bash
claude-scriptorium serve <session-id>.jsonl
```

Claude Code stores transcripts under `~/.claude/projects/`, one directory per
project path, one JSONL file per session. Set `CLAUDE_CONFIG_DIR` to read from
somewhere other than `~/.claude`.

## What gets rendered

Assistant and user turns, in order, with:

- Text and thinking as markdown, with GFM tables, task lists, and autolinks
- Fenced code highlighted into classed spans by
  [syntect](https://github.com/trishume/syntect), so the theme owns the colors
- Tool calls and their results as collapsible **marginalia**, labelled with the
  tool name and the subject of the call
- Pasted images inlined as data URLs
- A **colophon** footer recording what wrote the folio, and when

Turns a subagent produced carry `data-sidechain`, and turns the harness injected
carry `data-meta`, so a stylesheet can treat them differently.

Transcript content is escaped, never executed: a session that discusses
`<script>` renders it as text.

### Unrecognized content

Claude Code's transcript format grows new block types over time. An
unrecognized block renders as formatted JSON rather than aborting the folio,
because a new block type is a producer adding something optional, not malformed
input. Lines that carry no conversation at all (attachments, hook output, mode
changes, file-history snapshots) are skipped.

## Vocabulary

The code names things after the scriptorium that produced manuscripts by hand:

| Term | Meaning |
| --- | --- |
| folio | One rendered session |
| quire | The gathering of folios belonging to one project |
| codex | A full archive across projects |
| marginalia | A collapsible tool call or result |
| colophon | The generation-metadata footer |
| illumination | The theme layer |

## Development

[`just`](https://just.systems) drives the project tasks, and `just --list`
shows them all. After cloning:

```bash
just setup   # installs nightly rustfmt and the pre-commit hook
```

```bash
just check   # format, lint, and test
just test
just render <session>
```

Formatting runs under nightly rustfmt, since `rustfmt.toml` uses unstable
options. Committing runs the same formatting and linting through pre-commit.
