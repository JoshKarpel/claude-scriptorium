#!/usr/bin/env just --justfile

set dotenv-load
set ignore-comments

pre-commit-args := ""
cargo-test-args := ""
node-test-args := ""
watch-paths := "src Cargo.toml justfile"
bench-fixtures := "tests/fixtures/session.jsonl,tests/fixtures/playground.jsonl"
bench-warmup := "3"
huge-megabytes := "5 20"
profile-rate := "2000"
profile-iterations := "10"

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
    npm ci
    npx playwright install chromium

[doc("Run tests")]
[group("rust")]
test *args:
    cargo test --all-features {{ cargo-test-args }} {{ args }}

alias t := test

# The folio's own script is two files: a pure core, exercised here without a
# browser, and the shell that wires it to the document, exercised in one below.

[doc("Test the folio's script without a browser")]
[group("js")]
test-js *args:
    node --test {{ node-test-args }} "tests/js/*.test.mjs" {{ args }}

[doc("Drive a rendered folio in a headless browser")]
[group("js")]
test-browser *args:
    node --test {{ node-test-args }} "tests/browser/*.test.mjs" {{ args }}

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

# Optimized on purpose: a render is heavy enough (highlighting, base64'ing the
# fonts, megabytes of markup) that an unoptimized build takes longer to render
# than an optimized one takes to compile and render.

[doc("Render a session to an HTML file")]
[group("rust")]
render *args:
    cargo run --release -- render {{ args }}

alias r := render

[doc("Render a session and publish it as a gist")]
[group("rust")]
publish *args:
    cargo run --release -- publish {{ args }}

alias p := publish

[doc("Benchmark rendering the fixtures, e.g. `just bench --export-markdown bench.md`")]
[group("rust")]
bench *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v hyperfine >/dev/null 2>&1; then
      echo "just bench needs hyperfine (cargo install hyperfine / brew install hyperfine)" >&2
      exit 1
    fi
    cargo build --release -q
    out="$(mktemp -d)"
    trap 'rm -rf "$out"' EXIT
    # Timed through the binary rather than in-process, so a run measures the
    # whole artifact a reader waits on: parse, render, and write.
    hyperfine --warmup {{ bench-warmup }} --parameter-list fixture {{ bench-fixtures }} {{ args }} \
      "target/release/claude-scriptorium render {fixture} -o $out/"

alias b := bench

# The committed fixtures are short, where a session that runs for megabytes
# spends its time differently: a language's regexes compile once, so length
# amortizes what dominates a small render. These are written rather than
# committed, being millions of bytes of dealt-out fixture turns.

[doc("Write the large generated benchmark sessions under target/fixtures")]
[group("rust")]
fixtures:
    #!/usr/bin/env bash
    set -euo pipefail
    for megabytes in {{ huge-megabytes }}; do
      uv run scripts/synthetic_session.py --megabytes "$megabytes" \
        --output "target/fixtures/huge-${megabytes}mb.jsonl"
    done

[doc("Benchmark rendering the large generated sessions, e.g. `just bench-huge --runs 3`")]
[group("rust")]
bench-huge *args: fixtures
    #!/usr/bin/env bash
    set -euo pipefail
    sessions=""
    for megabytes in {{ huge-megabytes }}; do
      sessions+="${sessions:+,}target/fixtures/huge-${megabytes}mb.jsonl"
    done
    just bench-fixtures="$sessions" bench-warmup=1 bench --runs 5 {{ args }}

# perf and two kernel settings, written under /etc/sysctl.d so they survive a
# reboot (and, under WSL, a shutdown of the VM). `perf_event_paranoid` decides
# whether an unprivileged user may sample a process it owns at all;
# `perf_event_max_stack` is how many frames the kernel walks per sample, and its
# default of 127 truncates a render's deeply recursive regex compilation.

[doc("Install perf and the kernel settings `just profile` needs (needs sudo)")]
[group("rust")]
[linux]
setup-profiling:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! ls /usr/lib/linux-tools/*/perf >/dev/null 2>&1 && ! command -v perf >/dev/null 2>&1; then
      command -v apt-get >/dev/null 2>&1 \
        || { echo "install perf with your distribution's package manager, then rerun" >&2; exit 1; }
      sudo apt-get install -y linux-tools-generic
    fi
    printf '%s\n' 'kernel.perf_event_paranoid = 1' 'kernel.perf_event_max_stack = 1024' \
      | sudo tee /etc/sysctl.d/99-perf-event.conf > /dev/null
    sudo sysctl --system > /dev/null
    sysctl kernel.perf_event_paranoid kernel.perf_event_max_stack

# Sampled through the `profiling` build (release code plus the debug info a
# profiler needs to name a frame), over several iterations, since one render is
# over in a couple of hundred milliseconds and a hundred samples say nothing.
#
# Recorded by perf rather than samply's own sampler, and built with frame
# pointers for it to walk: samply unwinds a fixed 32 KB copy of each sample's
# stack and loses every frame above that, which here is four fifths of them, so
# a stack would root in the middle of regex compilation and the inclusive
# figures would undercount. perf walks the frame-pointer chain in the kernel
# with no such window. samply still reads the recording, for the report and for
# `samply load`.

[doc("Profile rendering a session and print a report, e.g. `just profile tests/fixtures/session.jsonl`")]
[group("rust")]
profile *args:
    #!/usr/bin/env bash
    set -euo pipefail
    for tool in samply uv; do
      command -v "$tool" >/dev/null 2>&1 || { echo "just profile needs $tool (see mise.toml)" >&2; exit 1; }
    done
    # Ubuntu's /usr/bin/perf is a wrapper that refuses to run when it has no
    # build matching the running kernel, which is every WSL kernel; the
    # versioned binary it declines to exec works fine.
    perf="$(command -v perf || true)"
    if [[ -z "$perf" ]] || ! "$perf" --version > /dev/null 2>&1; then
      perf="$(ls /usr/lib/linux-tools/*/perf 2> /dev/null | head -1)"
    fi
    [[ -x "${perf:-}" ]] || { echo "just profile needs perf: run just setup-profiling" >&2; exit 1; }
    session="{{ args }}"
    session="${session:-tests/fixtures/playground.jsonl}"
    RUSTFLAGS="-Cforce-frame-pointers=yes" cargo build --profile profiling -q
    out="$(mktemp -d)"
    trap 'rm -rf "$out"' EXIT
    mkdir -p target/profile
    render="target/profiling/claude-scriptorium render $session -o $(printf %q "$out")/ > /dev/null"
    "$perf" record --call-graph fp --freq {{ profile-rate }} --output target/profile/perf.data \
      -- bash -c "for _ in \$(seq {{ profile-iterations }}); do $render; done"
    samply import target/profile/perf.data --save-only --no-open \
      -o target/profile/profile.json > /dev/null
    uv run scripts/profile_report.py target/profile/profile.json \
      --binary target/profiling/claude-scriptorium --label "render $session" \
      | tee target/profile/report.txt
    echo >&2
    echo "report: target/profile/report.txt" >&2
    echo "flamegraph and timeline: samply load target/profile/profile.json" >&2

[doc("Serve one session, rebuilding and restarting on source changes")]
[group("rust")]
serve *args:
    @just reserve serve {{ args }}

alias s := serve

[doc("Serve every recorded session, rebuilding and restarting on source changes")]
[group("rust")]
codex *args:
    @just reserve codex {{ args }}

alias cx := codex

# A restart is how a change to the renderer, the stylesheet, or the app script
# reaches a reader: the server stamps every page with the run that set it, and a
# page told a different one reloads itself. So rebuilding and restarting here is
# the whole of the render loop.
[doc("Run a subcommand of the server, restarting it when sources change")]
[private]
reserve subcommand *args:
    #!/usr/bin/env bash
    set -uo pipefail
    if ! command -v fswatch >/dev/null 2>&1; then
      echo "just {{ subcommand }} needs fswatch (apt install fswatch / brew install fswatch)" >&2
      exit 1
    fi
    bin="target/release/claude-scriptorium"
    trap 'kill "${pid:-}" 2>/dev/null || true' EXIT
    while true; do
      pid=""
      if cargo build --release; then
        "$bin" {{ subcommand }} {{ args }} &
        pid=$!
      else
        echo "build failed; waiting for changes" >&2
      fi
      fswatch -1 {{ watch-paths }} >/dev/null
      [[ -n "$pid" ]] && kill "$pid" 2>/dev/null
      [[ -n "$pid" ]] && wait "$pid" 2>/dev/null
    done

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
    for tool in curl unzip uv uvx; do
      command -v "$tool" >/dev/null 2>&1 || { echo "just fonts needs $tool" >&2; exit 1; }
    done
    dir="{{ fonts-dir }}"
    lic="$dir/licenses"
    mkdir -p "$lic"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    get() { echo "  ${2##*/}" >&2; curl -sSfL "$1" -o "$2"; }

    # Upstream lands in $tmp; scripts/subset_fonts.py cuts it down into $dir,
    # since a folio inlines every face and upstream coverage is ~98% of one.
    echo "vendoring:" >&2

    # Junicode 2 (SIL OFL): humanist medievalist serif for body text. One
    # variable file per posture spans every weight; italic is a separate face.
    juni="https://raw.githubusercontent.com/psb1558/Junicode-font/{{ junicode-ref }}"
    get "$juni/webfiles/JunicodeVF-Roman.woff2" "$tmp/JunicodeVF-Roman.woff2"
    get "$juni/webfiles/JunicodeVF-Italic.woff2" "$tmp/JunicodeVF-Italic.woff2"
    get "$juni/OFL.txt" "$lic/Junicode-OFL.txt"

    # Fira Code (SIL OFL): monospace for code and tool output. The variable
    # woff2 ships only inside the release zip; its license lives in the repo.
    fira="https://github.com/tonsky/FiraCode"
    get "$fira/releases/download/{{ firacode-version }}/Fira_Code_v{{ firacode-version }}.zip" "$tmp/fira.zip"
    unzip -p "$tmp/fira.zip" "woff2/FiraCode-VF.woff2" > "$tmp/FiraCode-VF.woff2"
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
      -o "$tmp/UnifrakturCook.woff2" "$tmp/UnifrakturCook.ttf"

    echo "cutting down:" >&2
    uv run scripts/subset_fonts.py "$tmp" "$dir"

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
check: fix test test-js test-browser

alias c := check

[doc("Upgrade pre-commit hooks")]
[group("checks")]
pre-commit-upgrade:
    uvx pre-commit autoupdate
