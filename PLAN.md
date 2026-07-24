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
- **M2, own the markup and CSS.** Done. `src/illumination.css` is inlined
  into every folio's `<head>` as an illuminated-manuscript theme: a parchment
  leaf with gilt vine marginalia climbing the outer margins, rubricated
  small-caps headings, and fleuron dividers. The palette is real manuscript
  pigments (lapis lazuli, vermilion, malachite, Tyrian purple, ochre, gold)
  plus Claude orange for the assistant, held in CSS custom properties so the
  dark variant (M3) is a token swap. Syntax colors target the canonical
  TextMate scope roots (`ink-comment`, `ink-keyword`, `ink-string`, ...) that
  syntect emits as the first class on every span, so the palette covers every
  language without enumerating finer scopes.
- **M3, niceties.** Done. Dark mode is a `light-dark()` token swap driven by
  `color-scheme`, defaulting to the reader's system preference with a
  light/dark/system toggle that persists. Per-tool rendering gives `Bash`,
  `Write`, `Edit` (as a diff), and `TodoWrite` bespoke views, with the JSON
  fallback kept for everything else. These pulled in a trusted app script
  (`src/illumination.js`, inlined like the stylesheet) that also carries in-page
  search (highlight every match, step through with `‹ ›`/Enter), copy buttons on
  code blocks and messages, and a corner dock for jumping between turns and
  folding all tool calls at once. On wide screens an expanded tool call holding
  code unfurls past the reading measure so diffs fit without sideways scrolling.
  The self-contained invariant is now framed as gist-shareable bundle size, not
  script-freeness; transcript content is still escaped, never executed.
- **M4, CLI ergonomics.** Done. With no session named and a terminal
  attached, a two-stage `inquire` picker (project, then session) opens: the
  current project floats to the top and every list starts on its first row, so
  Enter, Enter lands on the current project's most recent session. Projects
  show the real working directory (recovered from the transcript, since the
  encoded directory name is lossy) and sessions show a relative time plus
  Claude's own `ai-title` (the summary it puts in terminal titles), read by a
  lenient `Folio::peek` that tolerates malformed lines. `--latest` resolves that
  same current-project/most-recent session non-interactively (the old default,
  now what CI and non-TTY invocations get, with a clear error otherwise). `-o`
  accepts a directory to write `<session-id>.html` into and creates missing
  parents; `--open` opens the written folio (or, for `serve`, the served URL).
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
  spans plus shipped CSS, so highlighting is baked in and works offline
  without a client-side highlighter.
- **Display panels:** raw turns are folded into a stream of `Panel`s
  (`Folio::panels`) before rendering. This is the single place for
  display-level filtering and grouping: tool-result turns (modelled as `user`
  on the wire) merge into the assistant turn that called the tool, `/clear`
  boundary turns drop out, and each panel is labelled by its content kind
  (`tool`, `thinking`, or the speaker) rather than a bare role.
- **Live-reload dev server:** the `serve` subcommand serves a session over
  HTTP and injects a reload snippet into the *served* response only (the
  written file carries the folio's app script but never the reload snippet).
  It reloads when the session file grows or
  the server restarts, so `just serve` (rebuild + restart on source change)
  gives a live edit loop. Uses `tiny_http`, no async runtime.
- **Other crates:** `clap`, `serde`/`serde_json`, `tiny_http` (serve),
  `inquire` for the two-stage picker, `open` for launching the browser.

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
