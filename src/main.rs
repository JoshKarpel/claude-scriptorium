use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use claude_scriptorium::{
    discovery,
    render::{Colophon, Scribe},
    serve,
    transcript::Folio,
};
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use jiff::{Timestamp, tz::TimeZone};

/// Render Claude Code sessions as self-contained HTML.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a session to a self-contained HTML file.
    Render(RenderArgs),
    /// Serve a session over HTTP with live reload for iterating on the render.
    Serve(ServeArgs),
}

#[derive(Args)]
struct RenderArgs {
    /// Session JSONL file. Defaults to the most recent session recorded for
    /// the current directory.
    session: Option<PathBuf>,

    /// Where to write the folio. Defaults to `<session-id>.html` here.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct ServeArgs {
    /// Session JSONL file. Defaults to the most recent session recorded for
    /// the current directory.
    session: Option<PathBuf>,

    /// Port to serve on.
    #[arg(long, default_value_t = 7878)]
    port: u16,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(args) => render(args),
        Command::Serve(args) => serve(args),
    }
}

fn render(args: RenderArgs) -> Result<()> {
    let session = resolve_session(args.session)?;
    let folio = Folio::read(&session)?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}.html", folio.session_id())));

    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());
    let markup = scribe.folio(&folio, &colophon());

    fs::write(&output, markup.into_string())
        .with_context(|| format!("writing {}", output.display()))?;
    println!("{}", output.display());
    Ok(())
}

fn serve(args: ServeArgs) -> Result<()> {
    let session = resolve_session(args.session)?;
    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());

    serve::run(args.port, &session, || {
        let folio = Folio::read(&session)?;
        Ok(scribe.folio(&folio, &colophon()).into_string())
    })
}

fn resolve_session(session: Option<PathBuf>) -> Result<PathBuf> {
    match session {
        Some(session) => Ok(session),
        None => {
            let cwd = std::env::current_dir().context("resolving current directory")?;
            let quire = discovery::quire_for(&discovery::projects_root()?, &cwd)?;
            Ok(quire.latest()?.to_path_buf())
        }
    }
}

fn highlighter() -> SyntectAdapter {
    SyntectAdapterBuilder::new()
        .css_with_class_prefix("ink-")
        .build()
}

fn colophon() -> Colophon {
    Colophon {
        generated: Timestamp::now(),
        tool: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    }
}
