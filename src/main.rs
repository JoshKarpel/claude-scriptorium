use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use claude_scriptorium::{
    discovery,
    render::{Colophon, Scribe},
    transcript::Folio,
};
use comrak::plugins::syntect::SyntectAdapterBuilder;
use jiff::{Timestamp, tz::TimeZone};

/// Render Claude Code sessions as self-contained HTML.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Session JSONL file. Defaults to the most recent session recorded for
    /// the current directory.
    session: Option<PathBuf>,

    /// Where to write the folio. Defaults to `<session-id>.html` here.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let session = match args.session {
        Some(session) => session,
        None => {
            let cwd = std::env::current_dir().context("resolving current directory")?;
            let quire = discovery::quire_for(&discovery::projects_root()?, &cwd)?;
            quire.latest()?.to_path_buf()
        }
    };

    let folio = Folio::read(&session)?;
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("{}.html", folio.session_id())));

    let highlighter = SyntectAdapterBuilder::new()
        .css_with_class_prefix("ink-")
        .build();
    let scribe = Scribe::new(&highlighter, TimeZone::system());
    let colophon = Colophon {
        generated: Timestamp::now(),
        tool: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    };

    let markup = scribe.folio(&folio, &colophon);
    fs::write(&output, markup.into_string())
        .with_context(|| format!("writing {}", output.display()))?;

    println!("{}", output.display());
    Ok(())
}
