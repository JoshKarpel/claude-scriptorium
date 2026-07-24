#!/usr/bin/env just --justfile

set dotenv-load
set ignore-comments

pre-commit-args := ""
cargo-test-args := ""
watch-paths := "src Cargo.toml justfile"

[default]
[doc("List available recipes")]
list:
    @just --list

[doc("Initial repo setup after cloning")]
setup:
    rustup toolchain install nightly --component rustfmt
    uvx pre-commit install
    uvx --from playwright playwright install chromium

[doc("Run tests")]
[group("rust")]
test *args:
    cargo test --all-features {{ cargo-test-args }} {{ args }}

alias t := test

# rustfmt.toml uses unstable options, which only nightly rustfmt honours.

[doc("Format Rust code")]
[group("rust")]
format:
    cargo +nightly fmt

[doc("Lint Rust code, fixing what clippy can fix on its own")]
[group("rust")]
lint:
    cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
    cargo clippy --all-targets --all-features -- -D warnings

[doc("Render a session to an HTML file")]
[group("rust")]
render *args:
    cargo run -- render {{ args }}

alias r := render

[doc("Serve a session with live reload, rebuilding and restarting on source changes")]
[group("rust")]
serve *args:
    #!/usr/bin/env bash
    set -uo pipefail
    if ! command -v fswatch >/dev/null 2>&1; then
      echo "just serve needs fswatch (apt install fswatch / brew install fswatch)" >&2
      exit 1
    fi
    bin="target/debug/claude-scriptorium"
    trap 'kill "${pid:-}" 2>/dev/null || true' EXIT
    while true; do
      pid=""
      if cargo build -q; then
        "$bin" serve {{ args }} &
        pid=$!
      else
        echo "build failed; waiting for changes" >&2
      fi
      fswatch -1 {{ watch-paths }} >/dev/null
      [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
      [[ -n "$pid" ]] && wait "$pid" 2>/dev/null
    done

alias s := serve

[doc("Rerun a recipe when sources change, e.g. `just watch render <session>`")]
[group("rust")]
watch recipe *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v fswatch >/dev/null 2>&1; then
      echo "just watch needs fswatch (apt install fswatch / brew install fswatch)" >&2
      exit 1
    fi
    run() { just {{ recipe }} {{ args }} || true; }
    run
    fswatch -o {{ watch-paths }} | while read -r _; do run; done

alias w := watch

[doc("Clean build artifacts")]
[group("rust")]
clean:
    cargo clean

[doc("Upgrade Rust dependencies")]
[group("rust")]
upgrade:
    cargo update

# Sequential on purpose: `cargo fmt` and `cargo clippy --fix` rewrite the same
# files, so running them concurrently races.

[private]
fix-code: format lint

[doc("Run pre-commit checks")]
[group("checks")]
fix:
    git add --update
    uvx pre-commit run {{ pre-commit-args }}

alias f := fix

[doc("Run all checks (formatting, linting, testing)")]
[group("checks")]
check: fix test

alias c := check

[doc("Upgrade pre-commit hooks")]
[group("checks")]
pre-commit-upgrade:
    uvx pre-commit autoupdate
