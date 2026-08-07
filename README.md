# claude-scriptorium

Render Claude Code sessions as self-contained HTML.

Each session becomes one **folio**: a single `.html` file with its markup,
styles, and images inlined. It can be mailed, gisted, or opened offline as one
file. Or read the whole **codex** in a browser, every session the machine has
recorded, with the live ones filling in as they are written.

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

Serve one session over HTTP and watch it as it is written (the same session
selection applies, so `serve --latest` or a bare `serve` picker both work):

```bash
claude-scriptorium serve <session-id>.jsonl
```

A served folio gains each panel in place as the session is written, so nothing
about the page you are reading is disturbed to add to it: your scroll position,
your open folds, and your search all stay put. Its dock therefore carries a
follow control that keeps the newest message pinned, like `tail -f`. A written or
published folio is a snapshot and never gains a message, so it offers no such
control.

### Browsing every session

`codex` serves the whole scriptorium: every project the machine has recorded, the
sessions in each, and any of them one click away as a folio. The session being
written right now is marked as such, so the one you are working in is on the
front page.

```bash
claude-scriptorium codex --open
```

It answers only this machine by default. To read it from elsewhere, bind it wider
and put something in front of it that says who may:

```bash
claude-scriptorium codex --host 0.0.0.0
```

Anything that can route to the machine can then read every session on it, so this
belongs behind an authenticating reverse proxy. On an
[exe.dev](https://exe.dev/docs/proxy) VM, for instance, the HTTPS proxy is private
by default and sends anyone without access to the VM to log in first, and it
forwards ports 3000 to 9999, so the codex above is at
`https://<vm>.exe.xyz:8000/` and nobody else's business.

`--root` lists a projects root other than Claude Code's own, and `--port`
chooses the port for either server, which defaults to 8000.

### Keeping a codex served

`cloister install` writes a systemd user service and starts it, so a machine
that records sessions serves them from the moment it boots, with nobody
attending it:

```bash
claude-scriptorium cloister install
```

It takes the same `--host`, `--port`, and `--root` as `codex`, and writes each
of them into the unit: a user service inherits nothing from the shell that
installed it, so a `CLAUDE_CONFIG_DIR` set in your session would otherwise mean
the service quietly served a different root than the one you asked for. The
`ExecStart` names the binary by absolute path, so install from a stable location
(`cargo install` puts it in `~/.cargo/bin`) rather than from a build tree that a
`cargo clean` will empty.

It is a *user* service rather than a system one, so it needs no root, and it
enables [lingering](https://www.freedesktop.org/software/systemd/man/loginctl.html)
so it outlives the session that installed it.

Re-running converges rather than starting over: the unit is rewritten only if it
differs, and only a changed unit restarts the service, so a re-run that changes
nothing leaves a reader's open folio and its event stream alone. `cloister
status` reports where it stands and what it binds, `cloister remove` undoes it,
and `--name` runs a second codex beside the first.

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

The embedded fonts put a folio near the gists API's ~1 MB read limit before a
session contributes anything, so any but the shortest clears it, and the API
answers a read of one with a raw URL rather than its contents. `fetch` therefore
clones the gist as the git repository it is. It needs `git` on the PATH, and uses
`gh` as git's credential helper for that one clone, so it needs no git
configuration of its own.

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
claude-scriptorium scaffold-viewer ./folio-viewer
claude-scriptorium scaffold-viewer ./folio-viewer --host ghe.example.com
```

The host defaults to the one `gh` publishes to, so a work machine scaffolds a
viewer for its own instance without being told; `--host` overrides it.

That writes a small git repo (a viewer `index.html`, its GitHub API base set to
the chosen host, plus a README with deploy steps). Push it, enable Pages, then
set `CLAUDE_SCRIPTORIUM_VIEWER_BASE` to your Pages URL. The viewer is vendored
from [GistHost](https://github.com/gisthost/gisthost.github.io) (MIT).

A viewer is a static page holding no credential, which is a limit worth knowing
before deploying one to an enterprise instance. It needs the instance to serve
the gists API to an anonymous browser, so an instance in private mode will turn
it away, and it needs the folio to be under the API's ~1 MB limit, or the API
answers with a raw URL the instance's web app will only open for a session
cookie. Where either holds, `fetch --open` is the way to read a folio, and it
works regardless.

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

A folio opens with a **caveat**, the one piece of the reading column this tool
wrote rather than set from the session. A session file isn't everything the
model was told: the system prompt, the tool descriptions, and the `CLAUDE.md`
and rule files loaded when a session starts are sent with every request but
never recorded, so no folio can show them. The caveat is not a panel, and
nothing that reads panels counts it as one: the dock won't step to it, the
minimap draws no band for it, the key can't set it aside, and the search never
returns it as a hit.

The rest of the reading column is pure transcript. The folio's title, facts, and
a **colophon** (what wrote it, when, and what the render cost) live in a plaque
you open from the corner, and the parchment leaf is bordered with illuminated
vines threaded with marginal **drolleries**: colourful little beasts (snail,
frog, cat, butterfly, stag, and more) seated among the vine at intervals,
cycling through the whole bestiary so a long folio keeps turning up new ones.

A folio is scrolled by a scroll: the scrollbar thumb is a written sheet of
parchment wound onto two turned rollers, which lengthens and shortens with the
document and lies on its side for a code block that runs off the edge. It is
never hidden or narrowed, being both the position indicator and a drag target,
and a reader in a high-contrast mode gets their own system's scrollbar instead.

Turns a subagent produced are marked as such, and take Tyrian purple across
every kind at once, so a sidechain reads as one whoever was speaking in it.

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

A gloss is a panel like any other, so the key names its kind, the search and the
minimap reach it, and the dock steps to it along whichever side of the exchange
it came from. Only `note`, the catch-all, stands aside from that walk. Its
content stays folded behind its summary line either way, so the reading column
stays the conversation while the context behind it stays a click away.

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

- **Set the key** at the head of the rail down the right: a chip per kind of
  panel, carrying that kind's own pigment, so it says what every edge in the
  margin means as well as which kinds you want. What reached the model runs down
  one column and what it produced down the other. Everything under it answers to
  it, so you say once what you're reading through.
- **Search** the session from the fixed box: matches are highlighted, and
  `‹ ›` or Enter steps through them, opening any collapsed fold that holds a
  hit. It looks only at the kinds the key leaves in play.
- **Press the light you want to read by**, in the corner: the sun for day, the
  candle for after dark. There is no toggle, because the lights are the control.
  Whichever is burning is the one in force, so by day the sun is up and the
  candle stands smoking, and after dark the moon hangs among its stars and the
  candle is lit. A small reset appears once you have chosen, to hand the choice
  back to your system; until then the folio follows your OS preference. The
  choice is remembered across visits.
- **Copy** any code block, a whole message, or a fold's prose (a skill's
  instructions, a rule, a plan, a subagent's prompt), from the button standing
  muted in its corner, and hear a quill take it down: the scratch is synthesized
  on the spot, so it costs the folio no bytes to carry.
- **Navigate** from the corner dock: step along either side of the exchange, the
  cool arrows seeking what reached the model and the warm ones what it produced,
  or the middle pair taking both. The arrows honour the key too, so narrowing it
  to skills walks you through the skills alone. Each step names the turn in the
  URL, so it is a link you can share and a place a reload returns you to.
  Collapse or expand every fold at once from the same dock.
- **Scrub the minimap** at the foot of the rail, drawn as the bound volume
  itself: the session shut and seen from above its front board, a band per
  message in that message's own pigment among the leaves, with your place on the
  leaf drawn over it. Drag along it to travel, and turn the wheel over it to zoom
  the map alone, so a long session's messages come apart far enough to pick one
  out without moving from where you are reading. It honours the key as well,
  fading what you have set aside.
- **Open the plaque** in the corner for the folio's title, facts, and colophon.

Whichever light is burning flickers or turns its rays and casts a faint glow
across the leaf, and holds still if you prefer less motion.

On wide screens an expanded tool call holding a diff or code block unfurls past
the reading column so wide content fits without sideways scrolling, while prose
stays in a narrow, legible measure.

## Vocabulary

The code names things after the scriptorium that produced manuscripts by hand:

| Term | Meaning |
| --- | --- |
| folio | One rendered session |
| quire | The gathering of folios belonging to one project |
| codex | The bound volume of every quire, which `codex` serves |
| cloister | The enclosure a codex is served from unattended, as a service |
| caveat | The folio's own note to its reader, at the head of the column |
| marginalia | A collapsible tool call or result |
| gloss | A note the harness wrote into the session, set as its own panel |
| key | Which kinds of panel are in play, and what each edge's pigment means |
| rail | The column of cards the key leads: search, dock, and minimap |
| drollery | A marginal creature drawn in the border |
| luminary | A light the folio is read by, and the control that chooses it |
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
just serve <session>  # one folio, rebuilding and restarting as you edit
just codex            # every folio, the same way
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
