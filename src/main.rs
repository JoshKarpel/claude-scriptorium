use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use claude_scriptorium::{
    discovery, picker,
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

/// How to choose the session when the user doesn't name a file. Shared by the
/// subcommands so `render` and `serve` resolve a session the same way.
#[derive(Args)]
struct Selection {
    /// Session JSONL file. With none given, pick one interactively; pass
    /// `--latest` for the current project's most recent session instead.
    session: Option<PathBuf>,

    /// Skip the picker and use the most recent session recorded for the
    /// current directory's project.
    #[arg(long)]
    latest: bool,
}

#[derive(Args)]
struct RenderArgs {
    #[command(flatten)]
    selection: Selection,

    /// Where to write the folio: a file, or a directory to write
    /// `<session-id>.html` into. Defaults to `<session-id>.html` here.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Open the rendered folio in the default browser.
    #[arg(long)]
    open: bool,
}

#[derive(Args)]
struct ServeArgs {
    #[command(flatten)]
    selection: Selection,

    /// Port to serve on.
    #[arg(long, default_value_t = 7878)]
    port: u16,

    /// Open the served folio in the default browser.
    #[arg(long)]
    open: bool,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(args) => render(args),
        Command::Serve(args) => serve(args),
    }
}

fn render(args: RenderArgs) -> Result<()> {
    let session = resolve_session(args.selection)?;
    let folio = Folio::read(&session)?;
    let output = output_path(args.output, &folio)?;

    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());
    let markup = scribe.folio(&folio, &colophon());

    fs::write(&output, markup.into_string())
        .with_context(|| format!("writing {}", output.display()))?;
    println!("{}", output.display());

    if args.open {
        open::that(&output).with_context(|| format!("opening {}", output.display()))?;
    }
    Ok(())
}

fn serve(args: ServeArgs) -> Result<()> {
    let session = resolve_session(args.selection)?;
    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());

    serve::run(args.port, &session, args.open, || {
        let folio = Folio::read(&session)?;
        Ok(scribe.folio(&folio, &colophon()).into_string())
    })
}

fn resolve_session(selection: Selection) -> Result<PathBuf> {
    if let Some(session) = selection.session {
        return Ok(session);
    }

    let cwd = std::env::current_dir().context("resolving current directory")?;
    let root = discovery::projects_root()?;

    if selection.latest {
        return Ok(discovery::quire_for(&root, &cwd)?.latest()?.to_path_buf());
    }
    if !std::io::stdin().is_terminal() {
        bail!("no session given: pass a file path or --latest (no terminal to pick from)");
    }
    picker::pick_session(&root, &cwd)
}

/// Resolves where to write the folio. A directory target (an existing directory
/// or one the caller points at with a trailing separator) receives a
/// `<session-id>.html` file; anything else is taken as the file path itself.
/// Missing parent directories are created either way.
fn output_path(output: Option<PathBuf>, folio: &Folio) -> Result<PathBuf> {
    let filename = format!("{}.html", folio.session_id());
    let path = match output {
        None => PathBuf::from(filename),
        Some(target) if is_directory_target(&target) => target.join(filename),
        Some(target) => target,
    };

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(path)
}

/// True when `-o` names a directory to write into rather than a file to write:
/// one that already exists, or a path the caller ended with a separator to
/// signal intent before it exists.
fn is_directory_target(target: &Path) -> bool {
    target.is_dir()
        || target
            .as_os_str()
            .to_string_lossy()
            .ends_with(std::path::MAIN_SEPARATOR)
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
