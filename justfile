#!/usr/bin/env just --justfile

set dotenv-load
set ignore-comments

pre-commit-args := ""
cargo-test-args := ""
watch-paths := "src Cargo.toml justfile"

fonts-dir := "src/fonts"
junicode-ref := "v2.226"
firacode-version := "6.2"
unifrakturcook-ref := "e98889587d96e21ad9d93030fe37ccd9f6c50995"

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

[doc("Vendor the embedded web fonts and their licenses from upstream")]
[group("rust")]
fonts:
    #!/usr/bin/env bash
    set -euo pipefail
    for tool in curl unzip uvx; do
      command -v "$tool" >/dev/null 2>&1 || { echo "just fonts needs $tool" >&2; exit 1; }
    done
    dir="{{ fonts-dir }}"
    lic="$dir/licenses"
    mkdir -p "$lic"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    get() { echo "  ${2#"$dir"/}" >&2; curl -sSfL "$1" -o "$2"; }

    # Junicode 2 (SIL OFL): humanist medievalist serif for body text. One
    # variable file per posture spans every weight; italic is a separate face.
    juni="https://raw.githubusercontent.com/psb1558/Junicode-font/{{ junicode-ref }}"
    get "$juni/webfiles/JunicodeVF-Roman.woff2" "$dir/JunicodeVF-Roman.woff2"
    get "$juni/webfiles/JunicodeVF-Italic.woff2" "$dir/JunicodeVF-Italic.woff2"
    get "$juni/OFL.txt" "$lic/Junicode-OFL.txt"

    # Fira Code (SIL OFL): monospace for code and tool output. The variable
    # woff2 ships only inside the release zip; its license lives in the repo.
    fira="https://github.com/tonsky/FiraCode"
    get "$fira/releases/download/{{ firacode-version }}/Fira_Code_v{{ firacode-version }}.zip" "$tmp/fira.zip"
    unzip -p "$tmp/fira.zip" "woff2/FiraCode-VF.woff2" > "$dir/FiraCode-VF.woff2"
    echo "  FiraCode-VF.woff2" >&2
    get "https://raw.githubusercontent.com/tonsky/FiraCode/{{ firacode-version }}/LICENSE" "$lic/FiraCode-OFL.txt"

    # UnifrakturCook (SIL OFL): single-weight blackletter for headings. Cyreal's
    # repo is the upstream and carries the license beside the font, but ships a
    # TTF, so compress it to woff2 here.
    uc="https://raw.githubusercontent.com/cyrealtype/UnifracturCook/{{ unifrakturcook-ref }}"
    get "$uc/UnifrakturCook-Regular-nohints.ttf" "$tmp/UnifrakturCook.ttf"
    get "$uc/OFL.txt" "$lic/UnifrakturCook-OFL.txt"
    echo "  UnifrakturCook.woff2" >&2
    uvx --with brotli --from fonttools fonttools ttLib.woff2 compress \
      -o "$dir/UnifrakturCook.woff2" "$tmp/UnifrakturCook.ttf"

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
