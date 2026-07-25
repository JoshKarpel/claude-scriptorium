# claude-scriptorium

Render Claude Code sessions as self-contained HTML.

Each session becomes one **folio**: a single `.html` file with its markup,
styles, and images inlined. It can be mailed, gisted, or opened offline as one
file.

## Install

```bash
cargo install claude-scriptorium
```

Or grab a prebuilt binary with
[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall), skipping the
compile:

```bash
cargo binstall claude-scriptorium
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

`--open` opens the rendered folio in your browser.
Pass `--output`/`-o` to render to a chosen path, or into a directory:

```bash
claude-scriptorium render ~/.claude/projects/-home-me-work/<session-id>.jsonl -o folio.html
claude-scriptorium render --latest -o folios/
```

Locally serve a session over HTTP with live reload, for watching a session
(the same session selection applies, so `serve --latest` or a bare `serve` picker both work):

```bash
claude-scriptorium serve <session-id>.jsonl
```

### Sharing a folio

Publish a folio to a GitHub gist via the [`gh`](https://cli.github.com/) CLI,
which handles authentication. It publishes as whichever account `gh` resolves:
set `GH_HOST` to target a different GitHub host, and switch the active account
with `gh auth switch` (`gh gist create` has no flags of its own for either).
`publish` confirms the resolved account before pushing, so you can abort if it is
not the one you meant, and reminds you that a secret gist is unlisted but still
readable by anyone with the URL:

```bash
claude-scriptorium publish <session-id>.jsonl
```

The same session selection as `render`/`serve` applies. A gist over ~1 MB
(every folio, once fonts are embedded) won't render inline on GitHub, so viewing
a published one takes one of the two paths below.

#### View offline

Download the gist's files and open the HTML locally, with no network rendering
in the loop:

```bash
claude-scriptorium fetch <gist-url-or-id> --open
```

`publish` prints this exact command for the gist it just created. It keeps the
browser viewer out of the loop, but the gist itself already lives on GitHub; for
a truly sensitive session, don't publish at all: `render` the folio and share the
HTML file directly.

#### View in a browser

`--preview` additionally prints a link to a **viewer** page that renders the
folio in a browser. The viewer is a small static page (this project's
[Pages site](https://joshkarpel.github.io/claude-scriptorium/) by default); the
reader's browser fetches the gist straight from GitHub's API and writes it into
the page, so the viewer's host never receives the transcript. It is off by
default and asks to confirm, so nothing is surfaced for browser viewing unless
you ask.

```bash
claude-scriptorium publish <session-id>.jsonl --preview
```

Point `--preview` at a different viewer with `--preview-base <url>`, or set
`CLAUDE_SCRIPTORIUM_VIEWER_BASE` to default it on a machine that always publishes
to the same one. A github.com gist falls back to this project's viewer; any
other host has no built-in viewer, so supply your own.

#### Self-hosting a viewer (and GHES)

For full control, or for a GitHub Enterprise instance (whose gists this
project's viewer can't reach), scaffold your own viewer site and serve it from
GitHub Pages:

```bash
claude-scriptorium scaffold-viewer ./folio-viewer               # for github.com
claude-scriptorium scaffold-viewer ./folio-viewer --host ghe.example.com
```

That writes a small git repo (a viewer `index.html`, its GitHub API base set to
the chosen host, plus a README with deploy steps). Push it, enable Pages, then
publish with `--preview-base <your Pages URL>`. The viewer is vendored from
[GistHost](https://github.com/gisthost/gisthost.github.io) (MIT).

Claude Code stores transcripts under `~/.claude/projects/`, one directory per
project path, one JSONL file per session. Set `CLAUDE_CONFIG_DIR` to read from
somewhere other than `~/.claude`.

## What gets rendered

Assistant and user turns, in order, with:

- Text and thinking as markdown, with GFM tables, task lists, and autolinks
- Fenced code highlighted into classed spans by
  [syntect](https://github.com/trishume/syntect), so the theme owns the colors
- Tool calls and their results as **marginalia**, labelled with the tool name
  and the subject of the call, and set in the shape that suits the tool: a
  command as highlighted shell, an edit as a diff, a plan or a subagent prompt
  as the markdown it was composed as, a read's result as the language of the
  file it read, a search's result as the links it found. A call its label
  already states in full is one flat line; everything else folds. A result that
  says only that the call was carried out is left out, since the call above it
  already shows what happened.
- Terminal colour kept: output written with ANSI escapes reads in the folio's
  own pigments, rather than showing the escapes
- Pasted images inlined as data URLs

The reading column is pure transcript. The folio's title, facts, and a
**colophon** (what wrote it, and when) live in a plaque you open from the
corner, and the parchment leaf is bordered with illuminated vines threaded with
marginal **drolleries**: colourful little beasts (snail, frog, cat, butterfly,
stag, and more) seated among the vine at intervals, cycling through the whole
bestiary so a long folio keeps turning up new ones.

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

Tools are treated the same way. A tool with no view of its own, such as one an
MCP server provides, shows the input it was sent as formatted JSON, and so does
a built-in whose input doesn't match the shape its view expects.

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
- **Navigate** from the corner dock: jump between user and assistant messages
  (skipping tool and thinking panels), and collapse or expand every tool call at
  once.
- **Open the plaque** in the corner for the folio's title, facts, and colophon.

On wide screens an expanded tool call holding a diff or code block unfurls past
the reading column so wide content fits without sideways scrolling, while prose
stays in a narrow, legible measure.

## Vocabulary

The code names things after the scriptorium that produced manuscripts by hand:

| Term | Meaning |
| --- | --- |
| folio | One rendered session |
| quire | The gathering of folios belonging to one project |
| marginalia | A collapsible tool call or result |
| drollery | A marginal creature drawn in the border |
| colophon | Generation metadata, shown in the plaque |
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
[UnifrakturCook](https://github.com/cyrealtype/UnifracturCook) for headings and
dropped initials.
All three are licensed under the
[SIL Open Font License 1.1](https://openfontlicense.org); their license texts
are vendored in `src/fonts/licenses/`. `just fonts` re-vendors the woff2 files
and licenses from those upstreams.
