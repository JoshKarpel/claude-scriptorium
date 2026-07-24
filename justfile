#!/usr/bin/env just --justfile

set dotenv-load
set ignore-comments

pre-commit-args := ""
cargo-test-args := ""

[default]
[doc("List available recipes")]
list:
    @just --list

[doc("Initial repo setup after cloning")]
setup:
    rustup toolchain install nightly --component rustfmt
    uvx pre-commit install

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

[doc("Render a session to HTML")]
[group("rust")]
render *args:
    cargo run -- {{ args }}

alias r := render

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
