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

A served folio re-renders as the session is written, so its dock carries a
follow control that keeps the newest message pinned, like `tail -f`. A written
or published folio is a snapshot and never gains a message, so it offers no
such control.

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

The same session selection as `render`/`serve` applies. GitHub shows a gist's
HTML as source rather than rendering it, whatever its size, so reading a
published folio takes one of the two paths below.

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

The gist page shows the folio's HTML source, not the folio, so `publish` also
prints a link to a **viewer** page that renders it. The viewer is a small static
page (this project's
[Pages site](https://joshkarpel.github.io/claude-scriptorium/) by default); the
reader's browser fetches the gist straight from GitHub's API and writes it into
the page, so the viewer's host never receives the transcript.

Point it at a different viewer with `--preview-base <url>`, or set
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
  already states in full is one flat line; everything else folds. Each result
  sits with the call it answers, and its line previews the first thing that came
  back rather than naming the call again: what a command printed, a file's
  opening line, the option that was chosen. A result that says only that the
  call was carried out is left out, since the call above it already shows what
  happened.
- Terminal colour kept: output written with ANSI escapes reads in the folio's
  own pigments, rather than showing the escapes
- Pasted images inlined as data URLs

The reading column is pure transcript. The folio's title, facts, and a
**colophon** (what wrote it, when, and what the render cost) live in a plaque
you open from the corner, and the parchment leaf is bordered with illuminated
vines threaded with marginal **drolleries**: colourful little beasts (snail,
frog, cat, butterfly, stag, and more) seated among the vine at intervals,
cycling through the whole bestiary so a long folio keeps turning up new ones.

A folio is scrolled by a scroll: the scrollbar thumb is a written sheet of
parchment wound onto two turned rollers, which lengthens and shortens with the
document and lies on its side for a code block that runs off the edge. It is
never hidden or narrowed, being both the position indicator and a drag target,
and a reader in a high-contrast mode gets their own system's scrollbar instead.

Turns a subagent produced carry `data-sidechain`, so a stylesheet can treat
them differently.

Transcript content is escaped, never executed: a session that discusses
`<script>` renders it as text.

### Glosses

Much of what shapes a session is written into it by the harness rather than
typed by anyone, and a reader who can't see it is left guessing why the
assistant did what it did. So each of those notes gets its own quiet panel, a
**gloss**, labelled by what wrote it there, with its content folded away behind
a summary line:

| Gloss | What it holds |
| --- | --- |
| `hook` | What a hook printed, injected, or failed with |
| `rule` | A `CLAUDE.md` or rule file pulled into context |
| `skill` | The instructions a skill carries, whether a command loaded it or the assistant reached for it |
| `command` | A slash command, its arguments, and what it printed |
| `plan` | Entering plan mode, and leaving it with or without a plan |
| `note` | A file edited outside the session or attached to it, a truncated read, a model swapped mid-conversation |

A command that works the harness rather than the conversation, `/copy` or
`/config` say, is left out: being told the last reply went to the clipboard
tells a reader nothing. The ones that change the conversation stay.

Glosses are never navigation targets, and searching them is its own scope, so
the reading column stays the conversation while the context behind it stays a
click away.

Every kind of panel carries its own pigment, along one axis: **warm is what the
model produced, cool is what reached it from outside.** So scrolling tells you
which side of the exchange you're passing before you read a label. The assistant
speaks in its own orange, reasons in that orange drawn back toward the ink, and
reaches for a tool in ochre; you speak in lapis, type a command in that lapis
drawn back toward the ink, and your skills and hooks arrive in teal and
malachite. A plan boundary is the exception, rubricated in vermilion because it
marks a division in the text rather than anything said in it, and the ambient
kinds stay in faint ink so the rest can stay loud.

### What a folio cannot show

A transcript records the conversation, not the instructions a session opened
with. The project's `CLAUDE.md`, your global one, and every rule with no path to
trigger on are assembled into the system prompt, which is never written to the
session file, so **no folio can show them**: what you see are the rules and
nested `CLAUDE.md` files the harness pulled in *as it worked*, one gloss each.
A session can therefore read as though the assistant knew something from
nowhere, and the standing instructions are where to look.

The same goes for anything else outside the file: a tool's own side effects, the
state of the repository, and whatever you told Claude in another session.

### Unrecognized content

Claude Code's transcript format grows new block types over time. An
unrecognized block renders as formatted JSON rather than aborting the folio,
because a new block type is a producer adding something optional, not malformed
input. The same goes for the harness's own notes: one whose shape the folio
doesn't recognize is left unset rather than breaking the render. Lines that
carry no conversation and no context (an inventory of the available tools or
skills, a restatement of the checklist, the harness's own timings, mode changes,
file-history snapshots) are skipped.

Tools are treated the same way. A tool with no view of its own, such as one an
MCP server provides, shows the input it was sent as formatted JSON, and so does
a built-in whose input doesn't match the shape its view expects.

## Reading a folio

A folio is interactive, driven by one small script inlined alongside the
styles, so the file stays a single self-contained artifact:

- **Set the key** in the corner: a chip per kind of panel, carrying that kind's
  own pigment, so it says what every edge in the margin means as well as which
  kinds you want. What reached the model runs down one column and what it
  produced down the other. Everything else answers to it, so you say once what
  you're reading through.
- **Search** the session from the fixed box: matches are highlighted, and
  `‹ ›` or Enter steps through them, opening any collapsed fold that holds a
  hit. It looks only at the kinds the key leaves in play.
- **Theme** it light, dark, or system; the default follows your OS preference
  and the choice is remembered across visits.
- **Copy** any code block, a whole message, or a fold's prose (a skill's
  instructions, a rule, a plan, a subagent's prompt), from the button that appears on
  hover, and hear a quill take it down: the scratch is synthesized on the spot,
  so it costs the folio no bytes to carry.
- **Navigate** from the corner dock: step along either side of the exchange, the
  cool arrows seeking what reached the model and the warm ones what it produced,
  or the middle pair taking both. The arrows honour the key too, so narrowing it
  to skills walks you through the skills alone. Each step names the turn in the
  URL, so it is a link you can share and a place a reload returns you to.
  Collapse or expand every fold at once from the same dock.
- **Scrub the minimap** at the foot of the rail, drawn as the bound volume
  itself: the whole session seen from the fore-edge, a band per message in that
  message's own pigment, with your place on the leaf drawn over it. Drag along it
  to travel, and turn the wheel over it to zoom the map alone, so a long
  session's messages come apart far enough to pick one out without moving from
  where you are reading. It honours the key as well, fading what you have set
  aside.
- **Open the plaque** in the corner for the folio's title, facts, and colophon.
- **Read by candlelight** after dark, or by the sun in the day: the luminary
  over the theme toggle flickers or turns its rays, and casts a faint glow
  across the leaf. It holds still if you prefer less motion.

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
| gloss | A note the harness wrote into the session, set as its own panel |
| key | Which kinds of panel are in play, and what each edge's pigment means |
| rail | The column of cards the key leads: search, dock, and minimap |
| drollery | A marginal creature drawn in the border |
| luminary | The candle or sun the folio is read by, and the light it casts |
| colophon | Generation metadata, shown in the plaque |
| illumination | The theme layer |

## Development

[`just`](https://just.systems) drives the project tasks, and `just --list`
shows them all. After cloning:

```bash
just setup   # nightly rustfmt, the pre-commit hook, and the browser tests' toolchain
```

```bash
just check        # format, lint, and every test suite
just test
just test-js      # the folio's own script, without a browser
just test-browser # a rendered folio, driven in a headless Chromium
just render <session>
just bench        # time rendering the fixtures, with hyperfine
just bench-huge   # the same, over generated sessions of megabytes
just profile      # sample a render and report where the time went, with perf and samply
```

The folio ships one inlined script, split into a functional core
(`src/illumination.core.js`) and the shell that wires it to the document
(`src/illumination.shell.js`), so the arithmetic behind its controls is tested
as plain values and only the wiring needs a browser.

Profiling needs perf and two kernel settings; `just setup-profiling` installs
them (once per machine, with sudo).

Formatting runs under nightly rustfmt, since `rustfmt.toml` uses unstable
options. Committing runs the same formatting and linting through pre-commit.

`mise.toml` carries the tools the recipes shell out to (`gh`, `hyperfine`,
`just`, `node`, `samply`, `uv`), so [mise](https://mise.jdx.dev) users get all
of them with `mise install`. The Rust toolchain comes from
`rust-toolchain.toml` instead, since rustup is what resolves the nightly
rustfmt. The only JS dependency is [Playwright](https://playwright.dev), used
by the browser tests; nothing it installs reaches a folio.

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
and licenses from those upstreams, and cuts them down.

The faces are almost the whole of a short folio, so each is vendored twice: as
upstream shipped it, and cut down to the Latin, punctuation, symbol, and
box-drawing blocks a transcript actually sets. A folio carries the cut faces,
about a fifth the bytes, unless its own text reaches a character the cut ones
dropped, in which case it carries the whole faces and says so on stderr. Pass
`--whole-fonts` to embed the whole ones regardless. Characters no face ever
carried, an emoji or a CJK ideograph, fall back to the reader's own fonts as
they always have.
