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

Pick a session interactively (a project list, then its sessions, each labelled
with Claude's own title for it):

```bash
claude-scriptorium render
```

The current project floats to the top and every list starts on its first row,
so pressing Enter twice renders the current project's most recent session. To
get that same session without the prompt (for scripts, or over SSH with no
terminal), pass `--latest`:

```bash
claude-scriptorium render --latest --open
```

`--open` opens the rendered folio in your browser. Render a specific session
file to a chosen path, or into a directory:

```bash
claude-scriptorium render ~/.claude/projects/-home-me-work/<session-id>.jsonl -o folio.html
claude-scriptorium render --latest -o folios/
```

Serve a session over HTTP with live reload, for watching a session or iterating
on the rendering (the same session selection applies, so `serve --latest` or a
bare `serve` picker both work):

```bash
claude-scriptorium serve <session-id>.jsonl
```

### Sharing a folio

Publish a folio to a GitHub gist (via the [`gh`](https://cli.github.com/) CLI,
which handles authentication). `gh gist create` has no host override, so it
publishes as whichever account `gh` resolves; `publish` resolves that account up
front and confirms it, along with the reminder that a secret gist is unlisted
but still readable by anyone with the URL:

```bash
claude-scriptorium publish <session-id>.jsonl
```

The same session selection as `render`/`serve` applies. A gist over ~1 MB
(every folio, once fonts are embedded) won't render inline on GitHub, so viewing
one means either downloading it or routing it through a rendering proxy.

To view a published folio offline with nothing but a browser, download its files
and open the HTML locally:

```bash
claude-scriptorium fetch <gist-url-or-id> --open
```

`publish` prints this exact command for the gist it just created. For a quick
link that renders in a browser without downloading, `--preview` additionally
prints a [githack](https://raw.githack.com/) proxy URL, after confirming that
the proxy fetches and caches the full transcript. It is off by default so
nothing leaves for a third party unless you ask, which matters for
sensitive sessions: prefer `fetch` for those.

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

## Reading a folio

A folio is interactive, driven by one small script inlined alongside the
styles, so the file stays a single self-contained artifact:

- **Search** the session from the fixed box: matches are highlighted, and
  `‹ ›` or Enter steps through them, opening any collapsed tool call that holds
  a hit.
- **Theme** it light, dark, or system; the default follows your OS preference
  and the choice is remembered across visits.
- **Copy** any code block, or a whole message, from the button that appears on
  hover.
- **Navigate** from the corner dock: jump between turns, and collapse or expand
  every tool call at once.

On wide screens an expanded tool call holding a diff or code block unfurls past
the reading column so wide content fits without sideways scrolling, while prose
stays in a narrow, legible measure.

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

## Fonts

Each folio embeds its typefaces so it renders identically anywhere with no
network fetch:
[Junicode](https://github.com/psb1558/Junicode-font) for body text,
[Fira Code](https://github.com/tonsky/FiraCode) for code, and
[UnifrakturCook](https://github.com/cyrealtype/UnifracturCook) for headings.
All three are licensed under the
[SIL Open Font License 1.1](https://openfontlicense.org); their license texts
are vendored in `src/fonts/licenses/`. `just fonts` re-vendors the woff2 files
and licenses from those upstreams.
