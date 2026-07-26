# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

[`just`](https://just.systems) drives everything; `just --list` shows all
recipes.

```bash
just setup             # nightly rustfmt, pre-commit hook, Playwright Chromium (after cloning)
just check             # format, lint, then test
just test              # cargo test --all-features
just test <name>       # single test, e.g. just test markdown_becomes_html
just render <session>  # write a session to HTML (CLI `render` subcommand)
just serve <session>   # live-reload dev server, rebuilds on source change
just publish <session> # render a session and push it to a gist (CLI `publish` subcommand)
just bench             # hyperfine over the release binary rendering the fixtures
just bench-huge        # the same, over generated sessions of megabytes
just setup-profiling   # perf and the kernel settings just profile needs (once, needs sudo)
just profile <session> # sample a render and print where the time went
just fix               # pre-commit across the staged tree
```

`just render`, `just publish`, and `just serve` build with `--release`, and so
should any render you run by hand. A render is heavy enough (highlighting,
base64'ing the fonts, megabytes of markup) that an unoptimized build takes
longer to render a session than an optimized one takes to compile *and* render
it, so a debug render is
slower even counting the build. Tests still run unoptimized.

`just bench` times that binary with [hyperfine](https://github.com/sharkdp/hyperfine)
over the fixtures, so a run measures the whole artifact a reader waits on
(parse, render, write) rather than a library call. It is the check for a change
that claims to make rendering faster; take a reading before and after, since a
folio's own plaque reports a single run and says nothing about variance.

`just bench-huge` is the same measurement at length, over sessions of 5 and 20
MB that `scripts/synthetic_session.py` deals out from the committed fixtures'
own turns (`just fixtures` writes them under `target/fixtures`, and they are
generated rather than committed because they are millions of bytes of repeated
fixture). The two readings are not interchangeable, so take both. A short
session is dominated by compiling a language's highlighting regexes the first
time it is met; a long one has amortized that away and is dominated by the
per-byte work that follows, with thousands of panels to set rather than dozens.
A change can move one and leave the other alone.

`just bench` says a change is slower; `just profile` says what it is spending
the time on. It samples a render and prints a text report (self time by crate
and by symbol, time on the stack, and the hottest stacks) rather than opening a
UI, so the reading is something an agent can act on directly; the same recording
opens in the Firefox Profiler with `samply load target/profile/profile.json`
when a human wants the timeline and flamegraph. Both are measurements of the
whole binary, so read them together: a stack that is 40% of a profile is only
worth attacking if `just bench` says the render is slow enough to care.
`just setup-profiling` installs perf and writes the two kernel settings sampling
needs, once per machine.

It records the `profiling` cargo profile (release, plus the debug info a
profiler needs to name a frame, which is why it isn't just `release`) and runs
the render several times into one recording, since a single render is over in a
couple of hundred milliseconds and a hundred samples say nothing. The recorder
is `perf`, with the binary built under `-Cforce-frame-pointers=yes` so perf can
walk the frame-pointer chain in the kernel:
[samply](https://github.com/mstange/samply)'s own sampler unwinds a fixed 32 KB
copy of each sample's stack, and a render's stacks are deep enough in recursive
regex compilation that four fifths of them lost every frame above the window,
which quietly turned the inclusive figures into undercounts. The report leads
with the share of stacks that reached a common root, so that failure is visible
rather than inferred. samply still reads the `perf.data` (`samply import`), so
the report and the UI are unchanged.

`scripts/profile_report.py` folds that recording: it resolves the binary's own
addresses with `addr2line`, so frames inlined into an outer symbol are recovered
rather than charged to it (without that, an optimized render reads as little but
`main`), and names shared-library frames from the dynamic symbol table with
`nm`, sizes included, because a stripped system library has no debug info and
addr2line then answers with the nearest preceding exported symbol rather than
admitting it doesn't know: that is how a memmove variant came back as
`_dl_mcount_wrapper` and took 8% of a render with it. An address inside an
unexported function stays an address, and the caller above it in the stack
table is what says which one it is.

`just format` runs `cargo +nightly fmt`, not stable. `rustfmt.toml` sets
unstable options (`imports_granularity`, `group_imports`,
`reorder_impl_items`), which stable rustfmt ignores with a warning rather
than an error, so formatting silently diverges from CI if you run stable.

`rust-toolchain.toml` pins 1.96.0, but a `RUSTUP_TOOLCHAIN` environment
variable (mise sets one) overrides the file. The pin therefore takes effect in
CI and often not locally.

`mise.toml` carries the tools the recipes shell out to (`gh`, `hyperfine`,
`just`, `samply`, `uv`), so `mise install` is enough to run any of them. It
deliberately leaves the Rust toolchain to `rust-toolchain.toml`, since rustup
is what resolves the `+nightly` that `just format` needs. `fswatch`, which
`just serve` and `just watch` need, has no mise package and stays a system
dependency.

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
- `transcript`: parses JSONL lines into a `Folio` of raw `Recorded`s (a `Turn`
  of the conversation, or a `Gloss` the harness wrote into it), then folds those
  into a stream of display `Panel`s (`Folio::panels`). `Folio::peek` is a
  separate, deliberately lenient scan of a session's listing metadata (its
  `ai-title` and working directory) that tolerates malformed lines, because a
  picker label is best-effort where a render is strict.
- `picker`: the interactive two-stage selector (project, then session) the
  shell opens when no session is named and a terminal is attached.
- `render`: `Scribe` turns a folio's panels into `Markup`.
- `tools`: how each built-in tool's call and result are set, which is the one
  place that knows anything about a specific tool's shape.
- `gloss`: the counterpart for what the harness writes into a session (hook
  output, the rule files it pulls in, a skill's instructions, a slash command,
  the plan-mode boundaries), which is the one place that knows anything about a
  specific injection's shape.

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

That purity is also what makes a render parallel: `Scribe::folio` sets the
panels with rayon before writing the document around them, collected in order so
the folio reads the same. It is worth having because of where a render's time
goes. Highlighting is around nine tenths of it, and nearly all of that is a
syntax's regexes compiling the first time its language is met (syntect compiles
them lazily, through a `once_cell::sync` cell, so the whole session shares one
compile per pattern and a thread that wants a pattern another is already
compiling waits for it rather than repeating it). Sequentially, meeting a new
language stops everything; in parallel, the languages compile alongside each
other. A `Scribe` that read the clock or held mutable state would take that away
as surely as it would break the tests.

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

**Nothing on GitHub renders a folio, at any size.** The gist page shows its
HTML source, and the raw URL is served `text/plain; charset=utf-8` with
`x-content-type-options: nosniff`, so a browser will not execute it there
either. Browser viewing therefore always goes through a **viewer**: a ~6 KB
static page (`docs/index.html`, vendored from GistHost under MIT) whose script
fetches the gist from the GitHub API in the reader's browser and
`document.write`s it. The transcript's path is GitHub to the reader; the
viewer's host never receives it, unlike a re-serving proxy. That one file is
both served from this project's own Pages site and `include_str!`'d into the
binary, so `scaffold_viewer` emits a self-hostable copy from the same source
(rewriting the API base for a GHES host).

GitHub's ~1 MB limit is a real thing but a *different* thing, and the two are
easy to conflate: it governs whether the API returns the file's content or
truncates it, which the viewer already handles by falling back to the raw URL.
It has nothing to do with whether a folio can be viewed, and no folio is ever
readable without the viewer or `fetch`. So `publish` always prints the preview
link and says outright that the gist page shows source. Don't reintroduce a
size condition here, and don't describe the limit as an "inline-render cutoff".

The viewer base comes from `--preview-base`, then `$CLAUDE_SCRIPTORIUM_VIEWER_BASE`,
then this project's viewer for github.com; a host with no viewer (a GHES
instance without `--preview-base`) simply prints no link. `fetch` stays the
no-network-rendering path for sensitive sessions. The pure helpers (host
precedence, identity parsing, URL and viewer construction) are unit-tested; the
`gh`-shelling and prompts stay in the shell.

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
`assistant` become turns, `attachment` and `system` lines become turns or
glosses, and `#[serde(other)]` collapses everything else (mode changes,
file-history snapshots, and whatever gets added later) into `Bookkeeping` and
drops it. `Folio::read` produces a `Vec<Recorded>`, each either a `Turn` or a
`Glossed`, in file order, so a harness note keeps its place among the turns it
stands between.

An `attachment` line is one of three things. A `queued_command` is a message the
user typed while the assistant was still working: the harness records it here,
not as a `user` turn, so it becomes a `User` turn that slots into the stream at
the point it was dequeued (after the tool results complete, before the next
assistant turn) and renders as a user panel interjecting mid-response. Most of
the rest are notes the harness wrote into the session, and become glosses (see
below). The remainder is scaffolding and is dropped.

**An attachment's body stays raw `Value` rather than a typed enum, and this is
deliberate.** Serde's `#[serde(other)]` only catches an unknown *tag*: a
recognised tag whose fields don't match is a hard error, which would abort the
whole render because a harness version changed one field's type. `gloss.rs`
reads every body leniently instead, exactly as `tools.rs` reads a tool's input,
so no shape the harness invents can break a folio.

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

**A result joins the panel holding its call, not whichever panel is newest.**
Calls issued together are written as one `assistant` line each sharing a
`message.id`, so they are several panels; taking the last one piles every result
onto the last call and leaves its siblings showing none. `panels` therefore
keeps a map from each call's id to the panel it landed in (`homes`) and places
each result there, falling back to the newest assistant panel for a result whose
call the session never recorded.

`Panel` is an enum: `Speech` (a speaker's contribution) or `Gloss` (a harness
note). The same pass also lifts the turns recorded in the user's role that
aren't the user into glosses, via `gloss::wrapped`, which answers three ways
rather than two: a note to set, *nothing* (a wrapper with nothing under it), or
`None` for the user actually speaking. The three-way answer is load-bearing.
The caveat that stands in front of a slash command is its own turn, and
collapsing "no note here" into "leave it alone" renders that caveat as the user
speaking, which is exactly what it isn't.

Each panel is labelled by its `kind` (`Panel::kind`): the speaker already has a
border colour, so the label names the content instead, `tool` or `thinking` when
that's all the panel carries, a gloss's own kind when it is one, otherwise the
speaker. The kind reaches the markup as `data-kind`, and the stylesheet keys
both the border pigment and the label colour off it, so **a new kind needs a
pigment as well as a label** or it silently inherits its neighbour's. The dock
steps only between `data-kind="user"` and `data-kind="assistant"`, so no new
kind may ever collide with those either.

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

**The subject goes in the gist and never in a note.** The summary line is a flex
row and the gist is the only part of it that can shrink (`overflow: hidden` is
what zeroes a flex item's automatic minimum size); `.marginalia__note` and
`.marginalia__outcome` are both `flex: none`, so a note holding a path drives the
line clean out of the fold and squeezes the gist to nothing on the way.

**A result's line previews what came back, and says nothing else.** Every panel
holds exactly one call and one result (one assistant line carries one call, and
`panels` puts each result in its own call's panel), so naming the tool or its
subject again would only repeat the box directly above. The line is therefore the
*hint*, `tools::hint`, in the gist. Success is unmarked: the box under the call
is what says the call was answered, and `error` is set only because a failure is
the exception, which is what makes the mark worth reading.

**A hint must show what the fold shows**, so `hint`'s match shadows `result`'s
arm for arm, fallbacks included. Where a view sheds something, the hint sheds it
too (a read's line-number gutter, the tag around a failure); where a view
recomposes, the hint is drawn from what the fold *leads with* (the first thing
chosen out of an `AskUserQuestion` sentence, a task's first fact) rather than
from the markup it arrived in. Taking the raw first line instead puts back onto
the summary exactly what the view took out, which is how a read's line came to
open on a bare `1`. Adding an arm to `result` means deciding its arm in `hint`.

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

That split decides the type, too. A marginalia is pitched below the reading size
because its summary line is a label and its other bodies are data, but
`.tool--prose` is set back at `1rem`: a skill's instructions or a plan are the
same kind of text as the conversation around them and are read rather than
scanned. It also carries its own copy button, since it has no `pre` for the
one that every code block gets. And **inline code wraps** (`overflow-wrap:
break-word` on `:not(pre) > code`), because a fold's body is its box and inline
code has no scroller of its own: one unbreakable token would otherwise run
straight out of the fold. Code inside a `pre` is deliberately excluded, since a
block scrolls and breaking its lines would misreport the source.

A result's setting takes the call it answers. The wire format names the tool
only on the call, so `Folio::panels` resolves each result's `tool_use_id`
against the session's calls and stamps it with an `Answered` (the tool's name,
and the path where the call had one). That field is `#[serde(skip)]`: it is
never on the wire, and it exists so the renderer walks a stream where every
result already knows what produced it. `tools::hint` keys on that same name to
decide what its summary line can honestly preview, and `tools::result` uses the
path to pick the language a read's contents are set in.

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

### Setting the glosses

`gloss.rs` is `tools.rs`'s counterpart for what the harness writes into a
session, and follows the same rules: it keys on the shape, every reading answers
`Option`, and an unrecognised shape is simply left unset. A `Gloss` is pure data
(a kind, a gist, notes, and the `Body`s it has to say, each either `Prose` or
`Plain`, since one firing of a hook can say more than one thing);
`gloss::setting` turns it into the same `Setting` a tool call produces, so both
go through one `render::marginalia` and read as one kind of thing on the page.
Harvest real transcripts before adding a kind, as with a tool view.

The six kinds name *what wrote the note*, not what it holds: `hook`, `rule`,
`skill`, `command`, `plan`, and `note` as the catch-all. Keeping the label set
small is deliberate, because the summary line is where the specific labelling
belongs, exactly as a tool's name and gist divide the work.

**A skill reads the same however it was loaded.** `gloss::meta` knows one by the
directory it opens on, but a *built-in* skill (`/review`, `/init`,
`/security-review`) has none on disk, so its instructions arrive as bare prose
and would fall through to `note`. The slash command standing directly in front of
them is what says otherwise, so `Folio::panels` calls `Gloss::ran_by` to relabel
and name it. This matters because a skill is not a command's payload: across the
corpus 55 were loaded by a command of the same name and **28 by the model itself
with no command at all**. Were the command-loaded ones folded away or left as
notes, the same event would take a different shape purely by who triggered it.

**A command is not a skill, and the two take different pigments.** A command is a
gloss by where it sits rather than by whose it is: the harness recorded it, but
the user typed it. So `--command-edge` is the user's own hue drawn back toward
the ink, exactly as `--thinking-edge` is the assistant's, while a skill takes the
teal. Both are cool, because neither is the model; see the palette below for why
that is the deciding question.

One firing of a hook writes several lines (what it decided, what it injected,
what it printed), and `Folio::panels` gathers them into one panel the same way
it gathers a tool result into the call it answers: the decision labels the
summary line and the injected context fills the fold, rather than two panels
each saying half of it. The key is `Gloss::firing`, and **it is the `toolUseID`
plus the hook's own command, not the id alone**: the id names the *event*, one
event runs every hook matching it, and keying on it alone folds four
`SessionStart` hooks into one panel claiming to be a single hook that ran four
commands. So `Firing::joins` is not equality: two notes join when the event
matches and no two *different* commands stand between them, which is what lets
the lines a hook wrote through the harness (they carry no command of their own)
join the hook's own line. `Gloss::absorb` then narrows the panel's firing to
that command, so a *third* hook of the same event can no longer join what is now
identified, and it stacks the bodies rather than keeping the first: one firing
that had two things to say says both, in the order it said them.

A slash command's output is gathered the same way: the harness records what a
command printed as its own `system` line, and `GlossPanel::gathers` joins it to
the panel holding the command. **`parentUuid` is not a semantic pointer.** It
chains every line to the one before it, attachments included, so it says "the
previous line" and not "the line I am about". What makes the join sound is that
the output is *always* the very next line (138 of 138 across the corpus) and that
`panels` only ever joins into the panel it just opened; the uuid is what confirms
the pairing rather than what establishes it. Don't reach for `parentUuid` to tie
two lines together where adjacency isn't already doing the work.

That adjacency is also why `panels` keeps `unset`: a command the folio leaves
unset (one that only works the harness, or a `/clear` boundary) is dropped, and
its output line has to go the same way, or the folio sets what `/copy` printed
while never mentioning `/copy`.

What a note has to say is what decides whether it is set at all, mirroring the
`ACKNOWLEDGEMENTS` drop: a note with neither a gist nor a body is dropped rather
than set as a bare line saying something ran. Four drops are worth knowing:

- A `hook_success` records `content` (what the hook contributed to the session)
  and `stdout` (what it printed), and for a hook that just prints context they
  are the same text twice, so only one is set. A hook answering in the **control
  protocol** prints JSON that contributes nothing itself; what it asked for
  arrives as the `hook_system_message` and `hook_additional_context` notes beside
  it, so the protocol JSON is dropped rather than set twice. `stderr` never
  reaches the model and is always worth setting.
- A `plan_mode` attachment repeats with `reminderType: "sparse"` while plan mode
  stays on. Only `"full"` marks the boundary.
- The image-scaling notice (`[Image: original …  Multiply coordinates …]`) is
  coordinate advice for the model, and the image itself sits beside it.
- A slash command that works the harness rather than the conversation
  (`HARNESS_CONTROLS`: `/copy`, `/config`, `/resume`, and the rest). The
  transcript records every slash command the same way, whether it injected a
  whole skill or only redrew the screen, so telling them apart takes a list.
  Extend it against real transcripts: dropping one that *did* inject something
  loses the reason a session changed course, and the terse ones are not alike
  (`/compact` rewrites the context, `/model` changes who answers, and both stay).

Environment inventory (`task_reminder`, `skill_listing`, `deferred_tools_delta`,
`agent_listing_delta`, `command_permissions`, `date_change`) and the harness's
own bookkeeping (`system`/`turn_duration`, `system`/`stop_hook_summary`, which
restates the hook attachments beside it) stay dropped: a reader can't act on any
of it.

The plan itself needs nothing special. An older `ExitPlanMode` carries the plan
inline as `input.plan`, which `tools::plan` already sets; a newer one writes it
to a file with `Write`, so the text is already in the folio as an ordinary write.
**The renderer must not read the plan file off disk**: `Scribe` is pure, and a
folio has to render the same for a session that ended a year ago on another
machine.

`tests/fixtures/glosses.jsonl` holds one line per kind plus the ones that must
stay unset, the way `playground.jsonl` does for tools. It also carries a speech,
tool, and thinking panel, so it is the swatch for the panel pigments: every edge
a folio can draw is in one render, which is the only way to judge whether they
tell each other apart. Keep it that way when adding a kind.

### Rendering invariants worth preserving

- **Self-contained and gist-shareable.** Everything the folio needs is inlined:
  no external CSS, JS, fonts, or image files, so the one written file works
  offline and travels as a single artifact. The delivery path is a GitHub gist,
  which never renders HTML at any size, so a shared folio is read through the
  `gist` module's viewer (or downloaded with `fetch`) rather than GitHub's own
  file view. The constraint to respect is therefore **total bundle size** (keep
  it within what a gist and viewer will serve), *not* scripts: the
  viewer `document.write`s the folio and runs its inlined JS. Interactive
  behaviour (search, copy, collapse, jump) lives in a trusted app script inlined
  the same way the stylesheet is; keep it small. The copy button's quill
  scratch answers to that size constraint too, which is why it is a few lines
  of Web Audio (a word's worth of strokes with the pen lifted between them,
  each grained noise under its own pressure envelope, trimmed to the band a dry
  point sounds in) rather than an embedded recording: a sample is the obvious
  reach and would cost tens of kilobytes in every folio. Do **not**
  reintroduce a "no scripts" rule, it was dropped deliberately; the live
  invariant below is that *transcript* content is never executed, which is a
  different thing. `serve`
  still injects its live-reload snippet only into the *served* response, never
  persisting it to the file.
- **Fonts embedded, licensed.** The three families (`Junicode` serif body,
  `Fira Code` mono, `UnifrakturCook` blackletter headings and versals) are woff2 vendored
  under `src/fonts/` and base64'd into `@font-face` data URIs by `build.rs`,
  which `render.rs` `include_str!`s from `OUT_DIR` as constants: the faces
  never change between renders, so no render pays to encode them, and adding or
  swapping a face means editing `build.rs`'s `FACES`. `just fonts`
  re-vendors them from pinned upstreams; it needs `uv` and `uvx` for
  `fonttools`, which compresses UnifrakturCook (upstream ships only a TTF) and
  cuts the other three down. All four are
  SIL OFL 1.1: the license texts live in `src/fonts/licenses/`, and every folio
  carries the copyright notice (a comment above `<html>`) plus a colophon credit
  (in the folio's plaque), so each artifact satisfies the OFL's redistribution
  terms on its own.
- **Cut faces, with the whole ones behind them.** The faces are ~98% of a short
  folio, so `scripts/subset_fonts.py` (run by `just fonts`) vendors each one
  twice: as upstream shipped it in `src/fonts/`, and cut down in
  `src/fonts/cut/`. The cut pins the axes the stylesheet never varies (Junicode
  is variable on width and ENLA as well as weight; only weight is ever set) and
  subsets to the blocks a transcript sets, which is ~80% off. `build.rs` encodes
  both into `font-faces-cut.css` and `font-faces-whole.css`.

  Which one a folio carries is decided per folio, and the rule is that **cutting
  must never render a character worse than upstream would**. `dropped.txt`
  therefore records the *regression* (what a face had upstream and lost in the
  cut), not the cut faces' coverage: `build.rs` compiles it into `DROPPED`, and
  `Scribe::folio` weighs each panel against it as the panel is set, on the rayon
  threads already running. Any hit and the folio carries the whole faces
  instead, and the shell says so on stderr. A character *no* face ever carried
  (an emoji, a CJK ideograph) is deliberately absent from `dropped.txt`: it fell
  back to the reader's own fonts before the cut existed and still does, so
  growing the folio by 2 MB would buy nothing. `--whole-fonts` forces the whole
  faces for a folio that will later gain text this session did not have.

  `render::beyond_cut` skips runs of bytes under `0x80` without decoding them,
  which is only sound while nothing below that can be dropped; the tests hold
  that, and hold the stylesheet and app script inside the cut faces too, since
  the scan weighs the transcript rather than this crate's own markup. Widening
  the cut means editing `KEEP` in `scripts/subset_fonts.py` and re-running `just
  fonts`; it is safe, since a codepoint a face never had is simply not kept, but
  it is only *cheap* for some blocks, so **price a block before adding it** by
  cutting with and without it and diffing the two faces' bytes. `KEEP` was
  chosen by measuring the corpus rather than by taste, and the measurement that
  decides is cost against how often the block is actually written: the two
  symbol blocks holding `⟨these⟩` and `⬆` cost well under a kilobyte between
  them and are in, while Latin Extended-B costs 54 KB in *every* folio to spare the rare
  session that quotes a tool's mangled names, and Cyrillic likewise, so both
  stay out. Leaving a block out is not a failure: the whole faces are the net
  under it, and paying for that net in every folio is the trade this cut exists
  to avoid. Beware measuring against a session that reached a character *because
  you were demonstrating the fallback in it*, which is evidence of nothing.
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
- **The scroll answers to the reader's system before the pun.** The scrollbar
  thumb is a sheet wound onto two rollers, drawn in layered gradients rather
  than an SVG data URI the way the drolleries are: a `background-image` can't
  reach the palette, so a drawn roller would need a second copy for the dark
  scheme, where gradients resolve `light-dark()` like everything else. Only
  `::-webkit-scrollbar` can draw it, and that is Blink/WebKit only, so Firefox
  takes `scrollbar-color` instead, kept behind `@supports not
  selector(::-webkit-scrollbar)` because setting the standard property makes
  Blink discard that element's pseudo-element rules outright. Three things are
  load-bearing and unobvious. Everything sits inside `@media not
  (forced-colors: active)`: a styled `::-webkit-scrollbar` is not repainted
  under forced colours, it is left *blank*, so a high-contrast reader loses
  the bar entirely unless it is handed back to the UA. The thumb's ink edge is
  what identifies it against the track, since parchment on parchment is
  1.18:1 against the 3:1 that WCAG 1.4.11 asks; the writing is decorative and
  the sheet can't carry it. And the bar is never hidden and never narrowed
  below the platform default (`scrollbar-width: thin` is 10px against Blink's
  15px), because it is both the position indicator and a drag target. A test
  guards all three.

### The palette: warm is the model, cool is what reached it

**One axis decides every edge in the folio.** Warm hues are what the model
produced; cool hues are what reached it from outside. A reader scrolling learns
which side of the exchange they are passing before reading a single label, and
**a new kind has its side decided for it rather than chosen by taste**. This is
the first question to answer when adding one.

| Warm: the model's own | Cool: what reached it |
|---|---|
| `assistant` — `--claude` | `user` — `--lapis` |
| `thinking` — `--claude` toward the ink | `command` — `--lapis` toward the ink |
| `tool` — `--sienna` | `skill` — the teal |
| `plan` — `--rubric` | `hook` — `--verdigris` |
| | `rule` — the teal toward the ink |

A plan boundary is the model's: the mode is the user's to ask for, but entering
and leaving it is the model reporting on its own working, so it reads beside the
reasoning rather than beside the asking. It is rubricated rather than given a
hue of its own, because marking a division in the text is what a scribe ground
vermilion for. A rule is the user's own writing pulled into the conversation, so
it is theirs however the harness fetched it.

Three pairs are deliberately the same hue drawn back toward the ink rather than a
colour of their own, because none of them is a third party: reasoning is the
assistant's own voice unvoiced, a command is the user's own voice recorded as a
note, and a rule is a skill at lower volume (both are instruction files the user
wrote, but rules arrive a dozen at a time at the head of a session where a skill
arrives singly). `tool` takes the ochre a tool's *name* is already set in, so the
panel and the names inside it agree.

The cool side is ordered by how far from the reader each came: a command the user
typed, a skill they wrote (loaded by a command or by the model itself), a hook
answering on their behalf. Malachite sits furthest from a voice while still
being cool.

**A side is not a loudness.** Which side a kind is on decides *which way* it is
pitched; how far it is drawn back toward the ink is a separate call, and the
quiet kinds are the frequent ones. Deciding the side is compulsory; how loud to
be is not.

**One kind sits off the axis**, and only one: `note`, the catch-all, which by
definition has nothing in common to pitch. It stays in faint ink and is the only
kind the dock steps past. It is not vestigial, either: across a 60-session sample
it turned up 129 times against `rule`'s 141, almost all of them a file edited
outside the session while it ran, which is exactly the sort of thing a reader
needs and nothing else records.

`data-sidechain` is a third, orthogonal axis (whose turn it is, not what kind),
and takes Tyrian purple across every kind at once.

**The axis is declared once, in code.** `PanelKind::side` answers it and
`Scribe` writes the answer onto every panel as `data-side`, so the axis is
*recovered* by everything downstream rather than restated: the dock's flanking
arrows step along `[data-side]`, the search box groups its chips into a column
per side, and the stylesheet pitches each kind's pigment to match. `PanelKind::EVERY`
is the matching single declaration of the kinds themselves, which is what the
search chips are built from. The match in `side` is exhaustive on purpose, so
**a new kind will not compile until its side is decided** — and deciding it is
what then gives the kind its pigment, its chip, and whether the dock stops at it.
Don't add a list of kinds anywhere else; derive it from these two.

Markup carries `data-sidechain` for subagent turns so a stylesheet can
distinguish them. **Nothing in a folio is hidden with no way to reveal it.** A
gloss panel's edge is solid like any other's: it was dotted while the glosses
shared a few pigments and the edge had to say "nobody's speech" by itself, and
once every kind had its own hue that was a second mark for something already
said. The sidechain keeps its dashes, being a different axis. Each kind
sets the `--gloss-edge` token rather than `border-left-color`, so
`.turn[data-sidechain]`, which sets the colour outright, still wins and a
subagent's gloss still reads as a subagent. Its fold sheds its own frame while
shut so it reads as a line under the panel's header rather than a box inside a
box; opening it gives the frame back, since a fold holding output unfurls past
the reading measure and there would otherwise be nothing behind the text out
there. Those rules must sit *after* `.marginalia` in the stylesheet, or they
lose to it at equal specificity.

The reading column is pure transcript; the folio's chrome floats in the four
corners, all `position: fixed` and living in the shell of the markup rather than
the panel stream. Reading controls sit on the right, as a **rail**: one column of
cards led by the **key**, with the search and the navigation dock stacked under
it. They are in one column because they are one mechanism, the key governing the
two below it, and standing them together is what says so; appearance
sits on the left (a metadata plaque in the top corner revealing the title,
facts, and colophon on hover or focus; and the light/dark/system toggle bottom,
with the luminary standing over it). There is no in-column header or footer.

**The key is a control in its own right, not the search's.** It is a chip per
`PanelKind::EVERY`, each carrying its own kind's pigment so it doubles as the
legend for every edge in the margin. The chips are one grid of five rows filled
*by column*, so `EVERY`'s first half runs down the cool side and its second down
the warm one, rather than each side being nested in an element of its own. That
is what lets the halves be five and five with nothing stranded on a row alone,
which is why `note` sits at the foot of the warm column despite being
`Side::Aside`. **The order in `EVERY` is a layout, not a classification**: every
chip carries its own `data-side`, so the dock still steps past `note` and its
neutral ink still keeps it from reading as the model's. The search and
the dock both read it (`enabledKinds` in the app script), so a reader says once
what they are looking through rather than once per panel that looks: narrow the
key to `skill` and the search finds only skills *and* the arrows step only to
them. That is why it sits in the rail beside the search rather than inside it,
and why a minimap added later belongs in the same rail and needs no controls of
its own. The `aside` kinds stay out of the dock however the key is set, since the
arrows are the two sides and those kinds are on neither.

**The dock steps by writing the turn's permalink**, not by scrolling to it:
`history.replaceState` to `#turn-N`, then an instant `scrollIntoView`. All three
parts are load-bearing. The hash is what makes the position survive a reload,
which matters because `serve` re-renders under the reader and a scroll in flight
is simply lost when it does, with nothing recording where it was headed. The
landing is instant because an animation is a thing to wait out when the reader
means to press the button again. And it is `replaceState` rather than assigning
`location.hash` so that twenty steps don't cost twenty presses of Back;
`replaceState` performs no scroll of its own, which is why the scroll is
explicit. The deep-link handler already restores such a hash on load (and
releases follow, since a named turn is the reader taking control), so the dock
gets reload-correctness by joining that path rather than adding one.

The luminary (`luminary.svg`, inline in the appearance corner) is the light the
folio is read by: a guttering candle after dark, the sun with its rays circling
by day. Both are drawn into one box and every pigment is a `light-dark()` pair
whose off-scheme half is `transparent`, so the scheme lights one and puts the
other out with no second set of rules and no folio rendered for one scheme
alone. Its radiance, a very faint circle reaching across the leaf, is a sibling
inside the lamp rather than a fixed overlay, so it stays centred on the flame
wherever the corner puts it; it lifts the parchment by ~14/255 beside the lamp
and nothing at all at the far corner. Sunlight brightens what it falls on and a
candle warms it, so the day's wash is a pale warm white where the night's is the
flame's own amber: that amber over a light page stains rather than lights. All
of it stills under `prefers-reduced-motion`, which is what that query is for.

The dock's follow control (`tail -f`: re-pin the newest message's start on each
reload until the reader takes control by scrolling or by loading a `#turn-N`
deep link) is set only into a **served** folio, which is what `Delivery` on the
`Scribe` decides. `serve` re-reads the session and re-renders on every load, so
a served folio gains messages under its reader; a written or published one is a
snapshot of a session that may have ended a year ago, and following it would
promise an update that can never come. The control's *presence* is what tells
the app script this folio can follow, so there is one source of truth rather
than a flag in each: no control means jumping to the end stays a jump, and no
follow state is stored.

What the app script remembers is split by what it belongs to. The theme is the
reader's, and holds across everything they open, so it keeps one key. Which
marginalia stand open and whether the reader is following the end are facts
about one *folio*, so they are stored under the session id the markup names
(`data-folio` on `<body>`, which is the only reason that attribute exists): a
fold's own key is a turn number and a position within that turn, which names a
different marginalia in every session, and following a session still being
written says nothing about a folio finished months ago. An unscoped store is one
folio's state imposed on every other folio sharing the origin, and every folio a
reader opens from disk shares the `file://` origin, as does every folio served
through one viewer. Anything added that is a fact about the folio rather than
about the reader belongs under that same scope.

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
`Scribe` (the renderer), `Gloss` (a note the harness entered into a session, as
a later hand annotates a manuscript). Markup classes continue it with
`marginalia` (a collapsible tool call or result), `drollery` (a marginal
creature), `versal` (the dropped initial that opens a speaker's paragraph),
`key` (which kinds of panel are in play, and what each edge's pigment means),
`rail` (the column of cards the key leads), `luminary` (the candle or sun the
folio is read by, and its `radiance` over the leaf), and `illumination` (the
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
folio never exercises it. Confirming a change to the viewer means going through
a real gist. This publishes to a real account, so treat it as outward-facing:
get the user's go-ahead first, and delete the gist afterward.

The viewer has two paths through it, and the fixtures no longer cover both by
default: since the faces were cut, `session.jsonl` renders well under GitHub's
~1 MB API truncation limit, so it exercises only the inline-content path. Add
`--whole-fonts` (or publish a long session) to push a folio over the limit and
exercise the raw-URL fallback the truncated case takes.

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
