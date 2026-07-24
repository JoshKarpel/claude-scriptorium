# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

[`just`](https://just.systems) drives everything; `just --list` shows all
recipes.

```bash
just setup            # nightly rustfmt + the pre-commit hook (after cloning)
just check            # format, lint, then test
just test             # cargo test --all-features
just test <name>      # single test, e.g. just test markdown_becomes_html
just render <session> # cargo run against a session JSONL
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
- `transcript`: parses JSONL lines into a `Folio` of `Turn`s.
- `render`: `Scribe` turns a `Folio` into `Markup`.

`src/main.rs` is the imperative shell: it resolves paths, reads the clock and
system timezone, builds the syntect adapter, and writes the file. Everything
those decisions feed into is passed as an argument, so `Scribe` and the
markup functions stay pure and testable without mocks. Keep it that way when
adding features: a renderer that reads the clock or touches the filesystem
breaks the test suite's ability to assert on exact output.

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

### Rendering invariants worth preserving

- **Self-contained.** Everything the folio needs is inlined: no external CSS,
  JS, fonts, or image files. Adding an asset means inlining it.
- **Escaped, never executed.** Transcripts routinely contain `<script>` and
  raw HTML as subject matter. maud escapes interpolations and comrak escapes
  raw HTML by default; `tests/fixtures/injection.jsonl` guards this.
- **Classes, not colors.** Syntax highlighting goes through comrak's syntect
  plugin in class mode with an `ink-` prefix, so the stylesheet owns the
  palette. syntect is not a direct dependency; it arrives via comrak's
  `syntect-fancy` feature, which is the pure-Rust regex backend rather than
  the C oniguruma one.

Markup carries `data-sidechain` for subagent turns and `data-meta` for
harness-injected ones so a stylesheet can distinguish them.

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
  cargo run -q -- "$f" -o "/tmp/$(basename "$f" .jsonl).html" || echo "FAILED: $f"
done
```

Verify format claims against those files rather than from memory; `PLAN.md`
records what the survey found, including where earlier assumptions were wrong.

## Roadmap

`PLAN.md` holds the milestones and the decisions already settled (output
artifact, HTML generation, markdown renderer, highlighting). Read it before
proposing a different approach to any of them.
