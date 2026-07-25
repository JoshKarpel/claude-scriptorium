# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

[`just`](https://just.systems) drives everything; `just --list` shows all
recipes.

```bash
just setup            # nightly rustfmt, pre-commit hook, Playwright Chromium (after cloning)
just check            # format, lint, then test
just test             # cargo test --all-features
just test <name>      # single test, e.g. just test markdown_becomes_html
just render <session> # write a session to HTML (CLI `render` subcommand)
just serve <session>  # live-reload dev server, rebuilds on source change
just bench            # hyperfine over the release binary rendering the fixtures
just fix              # pre-commit across the staged tree
```

`just render` and `just serve` build with `--release`, and so should any render
you run by hand. A render is heavy enough (highlighting, base64'ing the fonts,
megabytes of markup) that an unoptimized build takes longer to render a session
than an optimized one takes to compile *and* render it, so a debug render is
slower even counting the build. Tests still run unoptimized.

`just bench` times that binary with [hyperfine](https://github.com/sharkdp/hyperfine)
over the fixtures, so a run measures the whole artifact a reader waits on
(parse, render, write) rather than a library call. It is the check for a change
that claims to make rendering faster; take a reading before and after, since a
folio's own plaque reports a single run and says nothing about variance.

`just format` runs `cargo +nightly fmt`, not stable. `rustfmt.toml` sets
unstable options (`imports_granularity`, `group_imports`,
`reorder_impl_items`), which stable rustfmt ignores with a warning rather
than an error, so formatting silently diverges from CI if you run stable.

`rust-toolchain.toml` pins 1.96.0, but a `RUSTUP_TOOLCHAIN` environment
variable (mise sets one) overrides the file. The pin therefore takes effect in
CI and often not locally.

When a change is user-visible (a new subcommand or flag, changed output, a bug
fix), add an entry to `CHANGELOG.md` under the current unreleased version,
grouped per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Architecture

One session JSONL in, one self-contained HTML file out. Three stages, one
module each:

- `discovery`: finds session files under `~/.claude/projects/`, where each
  project directory is named after its path with everything outside
  `[A-Za-z0-9_]` flattened to a dash. `CLAUDE_CONFIG_DIR` relocates the root.
  `all_quires` enumerates every project for the cross-project picker.
- `transcript`: parses JSONL lines into a `Folio` of raw `Turn`s, then folds
  those into a stream of display `Panel`s (`Folio::panels`). `Folio::peek` is a
  separate, deliberately lenient scan of a session's listing metadata (its
  `ai-title` and working directory) that tolerates malformed lines, because a
  picker label is best-effort where a render is strict.
- `picker`: the interactive two-stage selector (project, then session) the
  shell opens when no session is named and a terminal is attached.
- `render`: `Scribe` turns a folio's panels into `Markup`.
- `tools`: how each built-in tool's call and result are set, which is the one
  place that knows anything about a specific tool's shape.

`src/main.rs` is the imperative shell: it dispatches the `render`, `serve`,
`publish`, and `fetch` subcommands, resolves which session to show, reads the
clock and system
timezone, builds the syntect adapter, and writes the file. Session resolution
is layered: an explicit path wins, then `--latest` (the current project's most
recent session), then the interactive picker on a TTY, then a loud error when
there's no terminal to pick from. Everything the render decisions feed into is
passed as an argument, so `Scribe` and the markup functions stay pure and
testable without mocks. Keep it that way when adding features: a renderer that
reads the clock or touches the filesystem breaks the test suite's ability to
assert on exact output. Interactive I/O (the picker, browser-opening) lives in
the shell and its modules, never in the renderer.

A folio states what its own render cost, in the plaque's colophon. Neither
figure can be known while the markup is being written, so `Scribe::folio` leaves
an HTML-comment placeholder for each and `render::inscribe` fills them in once
the markup exists (a transcript can't forge one: raw HTML in transcript content
is escaped, so only the scribe's own markup reaches the document unescaped).
Substituting shifts the document's length, so the size a folio carries is the
markup it measured itself from rather than the file on disk, a difference the
human-readable figure rounds away. The shell measures the render alone, so the
figure means the same thing under every subcommand, and prints it (with the
finished document's exact size) on **stderr**, keeping stdout the folio's path
alone for a script to consume.

`serve` (`src/serve.rs`) is a dev-loop HTTP server: it re-renders on each page
load and injects a live-reload snippet so the browser refreshes when the
session file grows or the server restarts with fresh code. `just serve`
rebuilds and restarts it on source changes. The reload snippet is injected only
into the *served* response; the written file carries the folio's own app script
but never the reload snippet (see below).

`gist` (`src/gist.rs`) is the sharing path: `publish` renders a session and
pipes the HTML to a gist, `fetch` downloads a gist's files for offline viewing,
`scaffold-viewer` writes a self-hostable viewer site, and `gists`/`delete`
manage what this tool has published. It shells out to the `gh` CLI (its `gist`
porcelain, plus `gh api gists` where a JSON listing is needed) rather than
holding a GitHub token, so authentication, host selection, and account
resolution stay gh's job and no token lives in this code. `gh gist create` has
no `--hostname` and targets gh's default host, so `resolve_identity` recovers
that account (mirroring gh's own host precedence) and the shell confirms it
before pushing.

`publish` is idempotent per session. Every gist's description is stamped with a
marker (`GIST_MARKER`, this tool's package name) followed by the session id, and
its one file is named `<session-id>.html`. A republish finds the existing gist
by that filename and edits it in place, so the URL stays stable and re-running
doesn't pile up duplicates; visibility is fixed at creation, so a republish that
would flip secret/public fails loudly instead. The same marker is what makes a
gist recognisable as ours: `gists` lists only marked gists, and `delete` refuses
any gist whose description lacks the marker, so it can never remove something
this tool didn't publish. `delete --all` lists every marked gist and confirms as
a batch. Gists published before the marker existed carry an older description
and are deliberately not recognised.

A folio over GitHub's ~1 MB inline-render cutoff can't be viewed on the gist
page, so browser viewing goes through a **viewer**: a ~6 KB static page
(`docs/index.html`, vendored from GistHost under MIT) whose script fetches the
gist from the GitHub API in the reader's browser and `document.write`s it. The
transcript's path is GitHub to the reader; the viewer's host never receives it,
unlike a re-serving proxy. That one file is both served from this project's own
Pages site and `include_str!`'d into the binary, so `scaffold_viewer` emits a
self-hostable copy from the same source (rewriting the API base for a GHES host).
`publish` prints the preview link by default, with no opt-in flag or
confirmation: printing a link is harmless, and the accompanying note makes clear
only a reader's browser (never the viewer's host) fetches the transcript. Its
base comes from `--preview-base`, then
`$CLAUDE_SCRIPTORIUM_VIEWER_BASE`, then this project's viewer for github.com; a
host with no viewer (a GHES instance without `--preview-base`) simply prints no
link. `fetch` stays the no-network-rendering path for sensitive sessions. The pure helpers (host precedence, identity parsing, URL and
viewer construction) are unit-tested; the `gh`-shelling and prompts stay in the
shell.

`docs/index.html` is dual-purpose: this repo's own GitHub Pages viewer (Pages
serves from `main`'s `/docs`) and the template `scaffold_viewer` copies. Editing
it changes both, so keep the `API_BASE` line's exact `'https://api.github.com'`
literal intact: `scaffold_viewer` string-replaces it for GHES and fails loudly
if it's gone.

The crate is split into `src/lib.rs` plus a thin `src/main.rs` so integration
tests in `tests/` can import the modules. Adding a module means adding it to
`lib.rs`.

### Parsing is where the invariants get established

`Entry` is an internally-tagged enum over the JSONL `type` field. `user` and
`assistant` become turns; `#[serde(other)]` collapses everything else (hook
output, mode changes, file-history snapshots, and whatever gets added later)
into `Bookkeeping` and drops it.

`attachment` lines are the exception that isn't pure scaffolding: most are
(task reminders, hook output, memory), but a `queued_command` attachment is a
message the user typed while the assistant was still working. The harness
records it here, not as a `user` turn, so dropping all attachments silently
loses real conversation. `RawAttachment` lifts `queued_command` into a `User`
turn and drops every other attachment kind. These turns slot into the stream in
file order, at the point the queued message was dequeued (after the tool
results complete, before the next assistant turn), so they render as a user
panel interjecting mid-response.

One API response is written to the transcript a block at a time: several
`assistant` lines share a `message.id`, and every one of them repeats the whole
response's `usage`. Counting each line would multiply what a session cost, so
`Folio::read` keeps the usage on the first line carrying an id and drops it from
the rest. Anything else derived from usage must respect that: it is a fact about
a response, not about a line.

Usage is input and output tokens, and the words matter: "read" and "written" are
what tools do to files, not what a model does with a conversation. The input is
split by cache (fresh, cached this turn, replayed from an earlier one), which is
how it is *billed* and says nothing about the conversation, so `Usage::input`
recombines it. Every request re-sends the whole conversation, so a per-turn input
figure would restate the session rather than the turn: a panel shows
`Usage::uncached_input`, the part not served from cache, which is what the turn
added and what its own output stands against. Over a session, the output totals
(`Folio::output`) but the input is the largest single turn's
(`Folio::largest_input`), how big the conversation ever got, for the same reason.
The effort a turn ran at is recorded beside the message rather than inside it,
and only by harness versions that track it.

Content blocks parse as `Block::Known(...)` or fall through to
`Block::Unknown(Value)`, which renders as formatted JSON. This is deliberate
and load-bearing: Claude Code's transcript format grows new block types, and a
new one is a producer adding something optional, not malformed input, so it
must not abort the render. Real transcripts already contain a `tool_reference`
block nested inside tool results that no `Known` variant covers.

Strictness belongs only where a field is genuinely required. A line that isn't
JSON, or an assistant turn missing its `timestamp`, is a contract violation and
fails loudly with the file and line number.

### Folding turns into panels

`Folio::panels` is the one place display-level filtering and categorization
happen, so the renderer walks an already-clean stream and never re-derives any
of it. The wire format models a tool result as a `user` turn (it comes back in
the user role), but it isn't the user typing: `panels` merges each
tool-response turn's blocks back into the assistant turn that called the tool,
so a call and its result render inside one `Panel` (one bordered article) with
no intervening `user` heading. The same pass drops `/clear` boundary turns.
Add new turn-level filtering or grouping here, not in the renderer.

Each panel is labelled by its `kind` (`Panel::kind`): the border colour already
distinguishes user from assistant, so the label names the content instead,
`tool` or `thinking` when that's all the panel carries, otherwise the speaker.

A speech panel's leading paragraph opens with a rubricated versal (a dropped
blackletter initial). `Scribe::panel` finds the first visible-text block of a
`User`/`Assistant` panel and tags it `data-versal`; the stylesheet draws the
drop cap on that block's opening `<p>`, coloured by the speaker and uppercased
so a lowercased opener still gets a full-height capital. It is gilded with a
diagonally-lit gold-leaf silhouette that hugs the glyph: a ring of gold
`text-shadow`s stands in for a stroke, since `::first-letter` ignores
`text-stroke` (and `background-clip: text`), so the gilt cannot be a real
gradient and fakes the 135° sheen through directional shadow colours. The drop
is a uniform two lines, so a one-line message just sets a two-line minimum
height rather than leaving the initial dangling. Tool and thinking panels carry
no versal.

A marginalia is one fold: its summary line carries the labelling (the tool's
name, a gist of its subject, and any qualifier that changes what the call does),
and its body carries only the subject itself, filling the fold edge to edge. A
call and its result are therefore the same shape, and nothing sits in a second
box inside the first. A call its summary line already states in full has no
subject left for a body to hold, so it is set as one flat line with no fold to
open (`marginalia--flat`); a read is the common case, since the file and the
lines it took fit on the line and the contents arrive in the result. A flat
line's gist wraps where a folded one's ellipsises: truncation is only safe
where a fold can reveal what was cut, and a long search query or path would
otherwise be lost at the column's edge. The
stylesheet keys the body off `details > pre` rather than a class, because a
highlighted body is comrak's markup and can't carry one.

### Setting the tools

`tools.rs` owns every per-tool decision, keyed on the tool's name: `call`
returns a `Setting` (the summary line's gist, an optional link, any qualifying
notes, and the body), and `result` sets what came back. The shapes it keys on
are the harness's rather than a contract, so **every view returns `Option` and
falls back to pretty-printed JSON** when the input doesn't match. A tool that
grows a field, or an MCP server's tool that no view can know, must not break a
render. Harvest real transcripts (`~/.claude/projects/*/*.jsonl`) before adding
a view, rather than writing one from memory of a tool's parameters.

The body's shape follows what the subject *is*: prose that was composed as
markdown (a plan, a subagent prompt, a message, a skill's arguments) is set as
markdown through `prose`; code is a highlighted code block; a list of small
structured things (questions and their options, findings, todos) is its own
markup in the mono face.

A result's setting takes the call it answers. The wire format names the tool
only on the call, so `Folio::panels` resolves each result's `tool_use_id`
against the session's calls and stamps it with an `Answered` (the tool's name,
and the path where the call had one). That field is `#[serde(skip)]`: it is
never on the wire, and it exists so the renderer walks a stream where every
result already knows what produced it.

Naming a result is also what lets `panels` **drop** the ones that say nothing
their call doesn't: a write answering that the file was written, a skill
answering that it launched. The folio deliberately misrepresents the transcript
here, because a line saying an edit worked after every edit crowds out the
conversation. `ACKNOWLEDGEMENTS` matches the acknowledging *sentence* rather
than the tool, so a result carrying anything more survives; the corpus has edits
that acknowledge and then warn the file changed on disk, and that warning is the
only place a reader learns of it. A failure is never dropped. Match new
acknowledgements against real transcripts rather than from memory, since the
exact wording is what the drop keys on. A result made only of text blocks is
joined and read the same way a plain-text one is, since the blocks are how the
harness wrote it down rather than a difference in what came back; `spoken` is
that one reading, so a result is set and weighed as an acknowledgement off the
same text (a background agent's launch answers in blocks, and is dropped like
any other acknowledgement).

One result is *parsed* rather than just set: `AskUserQuestion` answers in a
sentence that names each question back before its answer, and the answer is what
a reader wants. `answers` recovers the pairs by anchoring on the `"=` between a
question and its answer and walking back from the *next* anchor to the `, "` that
opens the following question. Splitting forward on those separators looks
simpler and fails on a third of real transcripts: a question quotes code
containing quotes, a free-text answer runs to several clauses, and an option that
carried a preview has that preview echoed inline, so the delimiters all turn up
inside the values. Anything that doesn't parse (a question that timed out
answers in prose, with no pairs at all) falls back to the text as it came.

`tests/fixtures/playground.jsonl` holds one call-and-result pair per built-in
tool, harvested from real sessions, so a change to any view can be seen against
all of them at once: `just render tests/fixtures/playground.jsonl` (or `just
serve` it) and screenshot with every `details` forced open.
`tests/fixtures/answers.jsonl` is the same idea at one tool's depth: every shape
an `AskUserQuestion` result takes across the corpus (both opening sentences, a
missing closing one, a quoted option and typed prose, an echoed preview, several
selections joined into one answer, and a timeout), each with a test.

### Rendering invariants worth preserving

- **Self-contained and gist-shareable.** Everything the folio needs is inlined:
  no external CSS, JS, fonts, or image files, so the one written file works
  offline and travels as a single artifact. The delivery path is a GitHub gist,
  and a bundled folio (~3 MB, mostly the embedded Junicode faces) already
  exceeds GitHub's ~1 MB inline-render cutoff, so a shared folio is viewed
  through the `gist` module's viewer (or downloaded with `fetch`) rather than
  GitHub's own file view. The constraint to respect is therefore **total bundle
  size** (keep it within what a gist and viewer will serve), *not* scripts: the
  viewer `document.write`s the folio and runs its inlined JS. Interactive
  behaviour (search, copy, collapse, jump) lives in a trusted app script inlined
  the same way the stylesheet is; keep it small. Do **not** reintroduce a "no
  scripts" rule, it was dropped deliberately; the live invariant below is that
  *transcript* content is never executed, which is a different thing. `serve`
  still injects its live-reload snippet only into the *served* response, never
  persisting it to the file.
- **Fonts embedded, licensed.** The three families (`Junicode` serif body,
  `Fira Code` mono, `UnifrakturCook` blackletter headings and versals) are woff2 vendored
  under `src/fonts/` and base64'd into `@font-face` data URIs by `build.rs`,
  which `render.rs` `include_str!`s from `OUT_DIR` as one constant: the faces
  never change between renders, so no render pays to encode them, and adding or
  swapping a face means editing `build.rs`'s `FACES`. `just fonts`
  re-vendors them from pinned upstreams; it needs `uvx` because UnifrakturCook
  ships only a TTF and gets compressed to woff2 with `fonttools`. All four are
  SIL OFL 1.1: the license texts live in `src/fonts/licenses/`, and every folio
  carries the copyright notice (a comment above `<html>`) plus a colophon credit
  (in the folio's plaque), so each artifact satisfies the OFL's redistribution
  terms on its own.
- **Escaped, never executed.** Transcripts routinely contain `<script>` and
  raw HTML as subject matter. maud escapes interpolations and comrak escapes
  raw HTML by default; `tests/fixtures/injection.jsonl` guards this.
- **Classes, not colors.** Syntax highlighting goes through comrak's syntect
  plugin in class mode with an `ink-` prefix, so the stylesheet owns the
  palette. syntect is not a direct dependency; it arrives via comrak's
  `syntect-fancy` feature, which is the pure-Rust regex backend rather than
  the C oniguruma one. A terminal's colour follows the same rule: the sixteen
  colours ANSI *names* become `ansi--` classes the stylesheet grinds into the
  folio's pigments (`--ansi-*`), so a build log reads in the same palette as
  everything around it. The one exception is deliberate: a colour a tool states
  outright, as a 256-colour index outside the first sixteen or as 24-bit
  channels, is a value rather than a name, and no palette token can stand for
  it, so it is set on the element. Don't extend that exception to anything the
  palette *could* name.

Markup carries `data-sidechain` for subagent turns and `data-meta` for
harness-injected ones so a stylesheet can distinguish them. Meta turns are
command caveats, skill scaffolding, and context dumps, not conversation, so the
stylesheet hides them outright; they carry no reveal control, and the app script
skips them when searching so a match can't land in a permanently hidden panel.

The reading column is pure transcript; the folio's chrome floats in the four
corners, all `position: fixed` and living in the shell of the markup rather than
the panel stream. Reading controls sit on the right (search top, and a
navigation dock bottom that steps between user/assistant messages, skipping tool
and thinking panels, jumps to the end, follows new messages like `tail -f`
(re-pinning the newest message's start on each reload until the reader takes
control by scrolling or by loading a `#turn-N` deep link, state kept in
`localStorage`), and folds every marginalia); appearance
sits on the left (a metadata plaque in the top corner revealing the title,
facts, and colophon on hover or focus; and the light/dark/system toggle bottom).
There is no in-column header or footer.

The outer margins are illuminated borders. Each is a per-session strip of vine
sections with drolleries seated among them, composed in `render.rs`
(`margin_strip`): a PRNG seeded from the session id (with a per-side salt, so the
two borders differ) walks the cells, keeping most of them vine and seating a
drollery at the occasional non-seam, non-adjacent cell. Each seated drollery is
also mirrored horizontally at random (about the cell centreline x=45, so it stays
on the vine), so neither border faces a single consistent direction. Drolleries
are drawn from a shuffled bag of the whole bestiary that refills when drained, so
a border
cycles through every creature before any repeats rather than showing a fixed
few, and the strip is long enough (`STRIP_CELLS`) that all of them appear before
it recurs. The strip is base64'd into a data URI and set as an inline
`background-image` the stylesheet tiles with `repeat-y`, so one strip fills a
leaf of any height and re-tiles for free when a folio grows or a tool call
expands (which is why this is a background, not DOM cells that couldn't tile a
dynamic height). The cells live under `src/drolleries/` (one `.svg` each,
authored in a 90x210 box); a `background-image` SVG can't reach the palette
`var()`s, so their pigments are baked, chosen bright enough to read on either
parchment. Each drollery carries a measured `(dx, dy)` nudge (`DROLLERIES`) that
centres it in its cell: `dx` on the vine's line (x=45), `dy` in the gap between
the trail above and its mirror below (creatures are drawn low in the box, so
most lift toward the gap centre at y=105). The nudges are non-zero because a
tail or ear pulls the bounding box off centre. A drollery cell is framed by
`trail.svg`, a short vine stub baked above the creature and mirrored below, its
stroke fading to transparent (a `userSpaceOnUse` gradient shared by every trail)
as it nears the beast, so the vine dissolves in and coalesces back rather than
stopping at a gap.

**Centring a new drollery.** Don't eyeball the `(dx, dy)` nudge or hand-compute
the centre of curved, stroked paths: measure it. Render the creature's `.svg`
inside `<svg viewBox="0 0 90 210">` in a headless browser (the Playwright
Chromium `just setup` installs), read the content group's `getBBox()`, and set
`dx = round(45 - (bbox.x + bbox.width / 2))` and
`dy = round(105 - (bbox.y + bbox.height / 2))`, so the creature's bounding box
centres on the vine line (x=45) and the trail gap (y=105). The x=45 centreline
is where `vine.svg`'s path oscillates around; y=105 is the midpoint of the
trail gap, since `trail.svg` seats at y≈66 and its bottom mirror at y≈144. A
creature with a crest or ears (cockatiel, cardinal, hare) then sits with the
appendage reaching up toward the trail, which reads as intentional. This same
bbox-centre-on-(45, 105) recipe is what every existing nudge was set from, so
reuse it rather than introducing a second convention.

### Vocabulary

Types are named after a manuscript scriptorium, and the names are load-bearing
in the code: `Folio` (one rendered session), `Quire` (the gathering of folios
for one project), `Colophon` (generation metadata, shown in the plaque),
`Scribe` (the renderer). Markup classes continue it with `marginalia` (a
collapsible tool call or result), `drollery` (a marginal creature), `versal`
(the dropped initial that opens a speaker's paragraph), and `illumination` (the
theme layer).

## Testing against real data

Real sessions under `~/.claude/projects/*/*.jsonl` are where the transcript
format is *discovered*: verify any claim about it against those files rather
than from memory, and harvest them before writing a view (which fields a tool's
input carries, which wording a result comes back with, whether output arrives
with ANSI escapes in it).

They are not how the work is *checked*. Rendering every session takes minutes
and proves only that nothing panicked. When the corpus turns up a shape worth
handling, **write that shape into `tests/fixtures/` and assert on it**, so the
finding becomes a test that runs in seconds and keeps running. `playground.jsonl`
is where a tool's call and result go; the other fixtures cover one concern each.
A shape rare enough to be worth a comment is worth a fixture line.

### Visual verification

The stylesheet is the deliverable, so a styling change isn't done until it has
been looked at, not just asserted on in a string test. `just setup` installs a
Playwright-managed headless Chromium; render a folio and screenshot it:

```bash
cargo run -q --release -- render <session.jsonl> -o /tmp/folio.html
uvx --from playwright python - /tmp/folio.html /tmp/folio.png <<'PY'
import sys
from playwright.sync_api import sync_playwright
html, png = sys.argv[1], sys.argv[2]
with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page(viewport={"width": 1500, "height": 2600})
    page.goto(f"file://{html}")
    page.screenshot(path=png)
    browser.close()
PY
```

Read the PNG back to check the illumination. For an interactive loop, `just
serve <session>` reloads the browser as you edit the renderer or CSS.

### Verifying the gist viewer end to end

`docs/index.html` renders a folio in the browser by fetching the gist from the
GitHub API and `document.write`-ing it, so a `file://` screenshot of a rendered
folio never exercises it. Confirming a change to the viewer (or to a folio big
enough to trip GitHub's ~1 MB API truncation) means going through a real gist.
This publishes to a real account, so treat it as outward-facing: get the user's
go-ahead first, and delete the gist afterward.

1. `cargo run -q --release -- publish tests/fixtures/session.jsonl --yes` and capture the
   gist id from the printed URL.
2. Serve `docs/` over local **HTTP** (not `file://`, or the loader's `fetch`
   hits a null origin) and load `?<id>/<file>` in Playwright. `document.write`
   replaces the page *after* the API fetch resolves, so wait on a folio element
   (`.folio`) rather than `load`, then assert the folio is there and
   screenshot it:

```python
import functools, threading, sys
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from playwright.sync_api import sync_playwright

gid, docs_dir, png = sys.argv[1], sys.argv[2], sys.argv[3]
server = ThreadingHTTPServer(
    ("127.0.0.1", 0), functools.partial(SimpleHTTPRequestHandler, directory=docs_dir)
)
threading.Thread(target=server.serve_forever, daemon=True).start()
with sync_playwright() as p:
    page = p.chromium.launch().new_page(viewport={"width": 1500, "height": 2000})
    page.goto(f"http://127.0.0.1:{server.server_address[1]}/?{gid}/session.html")
    page.wait_for_selector(".folio", timeout=30000)
    assert page.title() == "folio session" and page.query_selector(".folio")
    page.screenshot(path=png)
```

3. `gh gist delete <id> --yes`, then read the PNG back to confirm the
   illumination rendered through the viewer, not just that the DOM is present.
