# claude-scriptorium

Rust CLI that renders Claude Code JSONL sessions into self-contained,
fully-custom HTML. The point of the project is owning the markup and CSS
top to bottom, so presentation is the deliverable, not an afterthought.

Crate name `claude-scriptorium` is confirmed available on crates.io
(hyphen and underscore forms both, they collide as one name).

## Why not just fork the existing tool

`simonw/claude-code-transcripts` (Apache-2.0, Python) does this well but
its styling is not customizable: CSS is a module-level constant injected
as `{{ css|safe }}`, templates load via Jinja `PackageLoader`, and there
is no `--css`/theme flag. Customizing means forking. Its real value is
the *schema knowledge*, not the code.

Use it as a reference for the JSONL shape (see `render_content_block`),
not as a source to copy. Apache-2.0 requires attribution if any code is
actually lifted.

## Input format (the domain knowledge worth stealing)

- Sessions live at `~/.claude/projects/<encoded-project-path>/<uuid>.jsonl`
- One JSON object per line. Keep entries where `type` is `user` or
  `assistant`; skip the rest. Each carries `timestamp` and `message`.
- The other entry types carry no conversation: `attachment`, `system`,
  `mode`, `last-prompt`, `ai-title`, `queue-operation`,
  `file-history-delta`, `file-history-snapshot`.
- `message.content` is either a plain string or a list of content blocks.
- Block types seen in the wild:
  - `text`: markdown
  - `thinking`: markdown, visually distinct
  - `tool_use`: `{name, input, id}`
  - `tool_result`: `{content, is_error}`; content is a string OR a list
    of nested blocks (`text`, `image`, `tool_reference`)
  - `image`: `source.media_type` plus base64 `source.data`
- Worth special-casing for nicer rendering: `TodoWrite`, `Write`, `Edit`,
  `Bash`. Everything else falls back to pretty-printed JSON.
- `isSidechain` marks turns a subagent produced; `isMeta` marks turns the
  harness injected. Both appear inline in the parent session.
- `isCompactSummary` appears nested inside content, not as a top-level
  entry flag, so finding compaction boundaries means digging for it.
- `agent-*.jsonl` files are subagent sessions; exclude by default.

## Design stance

Parse at the boundary into a typed enum, with an explicit catch-all
variant for unrecognized block types that renders as formatted JSON.
Claude Code's format drifts; a new block type is a producer adding an
optional thing, not malformed input, so it must not crash the render.
Strictness belongs only where a field is genuinely required.

## Milestones

- **M0, walking skeleton.** Done. Session discovery and typed JSONL
  parsing.
- **M1, ugliest possible HTML.** Done. One session to one self-contained
  `.html`, semantically structured and entirely unstyled. Syntax
  highlighting and collapsible tool calls came along for free, since both
  are markup rather than theme.
- **M2, own the markup and CSS.** The actual point. The classes are
  already emitted (`turn`, `block--thinking`, `marginalia`, `colophon`,
  `ink-*` for highlighted code, plus `data-sidechain` / `data-meta` /
  `data-error`), so this is writing the stylesheet and inlining it.
- **M3, niceties.** In-page search, dark mode, per-tool rendering for
  `TodoWrite`, `Write`, `Edit`, `Bash`.
- **M4, CLI ergonomics.** Interactive session picker, `--open`, output
  path handling.
- **M5, optional.** Gist publishing, browsable archive of all sessions.

## Decisions

- **Output artifact:** one self-contained `.html` per session, with CSS,
  JS, and images inlined. Trivial to share or gist, no asset paths to
  manage. An archive/index can layer on in M5 without disturbing the
  per-session renderer.
- **HTML generation:** `maud`. Markup lives in Rust beside the data it
  renders, auto-escaped and compile-time checked, which is what owning
  the markup top to bottom actually requires.
- **Markdown:** `comrak`, for full GFM (tables, task lists,
  strikethrough, autolinks) matching what Claude emits.
- **Syntax highlighting:** `syntect` at render time, emitting classed
  spans plus shipped CSS, so the artifact stays JS-free and offline.
- **Other crates:** `clap`, `serde`/`serde_json`, picker via `inquire`
  or `dialoguer` (M4), `open` for the browser (M4).

## Theming vocabulary

Monastic terms as internal concepts (crate names mostly taken, the ideas
are free): **colophon** for the generation-metadata footer, **marginalia**
for collapsible tool-call sidenotes, **illumination** for the theme layer,
**folio** for one rendered session, **quire** for the gathering of folios
belonging to one project, **codex** for a full archive.

## Ties back to dotfiles

The existing `bin/claude-transcript` wrapper (currently pinned to
`uvx claude-code-transcripts@0.6`, with a `--gist` account-confirmation
guard) can delegate to this once it exists. The `claude-` prefix matches
the existing `claude-*` bin family.
