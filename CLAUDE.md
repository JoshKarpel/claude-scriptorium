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
just fix              # pre-commit across the staged tree
```

`just format` runs `cargo +nightly fmt`, not stable. `rustfmt.toml` sets
unstable options (`imports_granularity`, `group_imports`,
`reorder_impl_items`), which stable rustfmt ignores with a warning rather
than an error, so formatting silently diverges from CI if you run stable.

`rust-toolchain.toml` pins 1.96.0, but a `RUSTUP_TOOLCHAIN` environment
variable (mise sets one) overrides the file. The pin therefore takes effect in
CI and often not locally.

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

`serve` (`src/serve.rs`) is a dev-loop HTTP server: it re-renders on each page
load and injects a live-reload snippet so the browser refreshes when the
session file grows or the server restarts with fresh code. `just serve`
rebuilds and restarts it on source changes. The reload snippet is injected only
into the *served* response; the written file carries the folio's own app script
but never the reload snippet (see below).

`gist` (`src/gist.rs`) is the sharing path: `publish` renders a session and
pipes the HTML to `gh gist create`, `fetch` downloads a gist's files for offline
viewing, and `scaffold-viewer` writes a self-hostable viewer site. It shells out
to the `gh` CLI rather than calling the GitHub API, so authentication, host
selection, and account resolution stay gh's job and no token lives in this code.
`gh gist create` has no `--hostname` and targets gh's default host, so
`resolve_identity` recovers that account (mirroring gh's own host precedence) and
the shell confirms it before pushing.

A folio over GitHub's ~1 MB inline-render cutoff can't be viewed on the gist
page, so browser viewing goes through a **viewer**: a ~6 KB static page
(`docs/index.html`, vendored from GistHost under MIT) whose script fetches the
gist from the GitHub API in the reader's browser and `document.write`s it. The
transcript's path is GitHub to the reader; the viewer's host never receives it,
unlike a re-serving proxy. That one file is both served from this project's own
Pages site and `include_str!`'d into the binary, so `scaffold_viewer` emits a
self-hostable copy from the same source (rewriting the API base for a GHES host).
The preview link is opt-in behind `--preview` and a confirmation; its base comes
from `--preview-base`, then `$CLAUDE_SCRIPTORIUM_VIEWER_BASE`, then this
project's viewer for github.com. `fetch` stays the no-network-rendering path for
sensitive sessions. The pure helpers (host precedence, identity parsing, URL and
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

`Entry` is an internally-tagged enum over the JSONL `type` field. Only `user`
and `assistant` become turns; `#[serde(other)]` collapses everything else
(attachments, hook output, mode changes, file-history snapshots, and whatever
gets added later) into `Bookkeeping` and drops it.

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
  `Fira Code` mono, `UnifrakturCook` blackletter headings) are woff2 vendored
  under `src/fonts/`, `include_bytes!`'d in `render.rs`, and base64'd into
  `@font-face` data URIs at render time (once, via a `LazyLock`). `just fonts`
  re-vendors them from pinned upstreams; it needs `uvx` because UnifrakturCook
  ships only a TTF and gets compressed to woff2 with `fonttools`. All four are
  SIL OFL 1.1: the license texts live in `src/fonts/licenses/`, and every folio
  carries the copyright notice (a comment above `<html>`) plus a colophon
  credit, so each artifact satisfies the OFL's redistribution terms on its own.
- **Escaped, never executed.** Transcripts routinely contain `<script>` and
  raw HTML as subject matter. maud escapes interpolations and comrak escapes
  raw HTML by default; `tests/fixtures/injection.jsonl` guards this.
- **Classes, not colors.** Syntax highlighting goes through comrak's syntect
  plugin in class mode with an `ink-` prefix, so the stylesheet owns the
  palette. syntect is not a direct dependency; it arrives via comrak's
  `syntect-fancy` feature, which is the pure-Rust regex backend rather than
  the C oniguruma one.

Markup carries `data-sidechain` for subagent turns and `data-meta` for
harness-injected ones so a stylesheet can distinguish them. Meta turns are
command caveats, skill scaffolding, and context dumps, not conversation, so
the stylesheet hides them by default; a pure-CSS checkbox (`#show-meta`, keyed
off `:has()`, rendered only when the folio has any) reveals them with no script
of its own, even though the folio now carries an app script for other features.

### Vocabulary

Types are named after a manuscript scriptorium, and the names are load-bearing
in the code: `Folio` (one rendered session), `Quire` (the gathering of folios
for one project), `Colophon` (generation-metadata footer), `Scribe` (the
renderer). Markup classes continue it with `marginalia` (a collapsible tool
call or result) and `illumination` (the theme layer).

## Testing against real data

`tests/fixtures/` holds hand-written JSONL covering each block type plus an
unrecognized one. For wider coverage, render every session on the machine and
check nothing fails:

```bash
for f in ~/.claude/projects/*/*.jsonl; do
  cargo run -q -- render "$f" -o "/tmp/$(basename "$f" .jsonl).html" || echo "FAILED: $f"
done
```

Verify format claims against those files rather than from memory; `PLAN.md`
records what the survey found, including where earlier assumptions were wrong.

### Visual verification

The stylesheet is the deliverable, so a styling change isn't done until it has
been looked at, not just asserted on in a string test. `just setup` installs a
Playwright-managed headless Chromium; render a folio and screenshot it:

```bash
cargo run -q -- render <session.jsonl> -o /tmp/folio.html
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

1. `cargo run -q -- publish tests/fixtures/session.jsonl --yes` and capture the
   gist id from the printed URL.
2. Serve `docs/` over local **HTTP** (not `file://`, or the loader's `fetch`
   hits a null origin) and load `?<id>/<file>` in Playwright. `document.write`
   replaces the page *after* the API fetch resolves, so wait on a folio element
   (`.colophon`) rather than `load`, then assert the folio is there and
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
    page.wait_for_selector(".colophon", timeout=30000)
    assert page.title() == "folio session" and page.query_selector(".folio")
    page.screenshot(path=png)
```

3. `gh gist delete <id> --yes`, then read the PNG back to confirm the
   illumination rendered through the viewer, not just that the DOM is present.

## Roadmap

`PLAN.md` holds the milestones and the decisions already settled (output
artifact, HTML generation, markdown renderer, highlighting). Read it before
proposing a different approach to any of them.
