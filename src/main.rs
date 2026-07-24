use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use claude_scriptorium::{
    discovery, gist, picker,
    render::{Colophon, Scribe},
    serve,
    transcript::Folio,
};
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use inquire::{Confirm, InquireError};
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
    /// Publish a rendered session to a GitHub gist via the `gh` CLI.
    Publish(PublishArgs),
    /// Download a published gist's files to view a folio offline.
    Fetch(FetchArgs),
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

#[derive(Args)]
struct PublishArgs {
    #[command(flatten)]
    selection: Selection,

    /// List the gist publicly instead of keeping it secret (the default).
    #[arg(long)]
    public: bool,

    /// Also print a preview link that renders the folio through a public
    /// third-party proxy (gist.githack.com). Off by default so nothing routes
    /// through a third party unless asked.
    #[arg(long)]
    preview: bool,

    /// Skip all confirmation prompts (for non-interactive use).
    #[arg(long)]
    yes: bool,

    /// Open the published folio in the default browser (the preview link when
    /// `--preview` is set, otherwise the gist page).
    #[arg(long)]
    open: bool,
}

#[derive(Args)]
struct FetchArgs {
    /// Gist id or URL to download.
    gist: String,

    /// Directory to write the gist's files into. Defaults to the current
    /// directory; missing parents are created.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Open the downloaded folio in the default browser, viewing it locally
    /// with no rendering proxy.
    #[arg(long)]
    open: bool,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(args) => render(args),
        Command::Serve(args) => serve(args),
        Command::Publish(args) => publish(args),
        Command::Fetch(args) => fetch(args),
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

fn publish(args: PublishArgs) -> Result<()> {
    let session = resolve_session(args.selection)?;
    let folio = Folio::read(&session)?;

    let identity = gist::resolve_identity()?;
    confirm_publish(&identity, args.public, args.yes)?;
    let include_preview = resolve_preview(&identity, args.preview, args.yes)?;

    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());
    let html = scribe.folio(&folio, &colophon()).into_string();

    let filename = format!("{}.html", folio.session_id());
    let description = gist_description(&session, folio.session_id());
    let gist_url = gist::publish(&html, &filename, &description, args.public)?;

    println!("{gist_url}");
    let preview = include_preview
        .then(|| gist::preview_url(&gist_url, &filename))
        .flatten();
    if let Some(preview) = &preview {
        println!("{preview}");
    }
    println!("anyone can view it locally, with no proxy, by running:");
    println!("  {} fetch {gist_url} --open", env!("CARGO_PKG_NAME"));

    if args.open {
        let url = preview.as_deref().unwrap_or(&gist_url);
        open::that(url).with_context(|| format!("opening {url}"))?;
    }
    Ok(())
}

/// Confirms publishing, naming the account it will publish as and the gist's
/// visibility. `gh gist create` targets gh's default host with no way to
/// override it, so on a multi-account machine the wrong identity is one Enter
/// away; and a secret gist is unlisted but not private. `--yes` skips the
/// prompt; a non-terminal with no `--yes` refuses rather than publishing blind.
fn confirm_publish(identity: &gist::Identity, public: bool, assume_yes: bool) -> Result<()> {
    if assume_yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("refusing to publish as {identity} without confirmation: pass --yes");
    }

    let visibility = if public { "public" } else { "secret" };
    println!("Publishing this session as a {visibility} gist, as {identity}.");
    if public {
        println!(
            "  A public gist is listed on {} and readable by anyone.",
            identity.host
        );
    } else {
        println!(
            "  A secret gist is unlisted, but anyone with access to {} and the URL can read it.",
            identity.host
        );
    }

    if !ask(&format!("Publish this {visibility} gist?"))? {
        bail!("aborted");
    }
    Ok(())
}

/// Decides whether to emit the third-party preview link. Requested with
/// `--preview`, it takes a second confirmation because the proxy fetches and
/// caches the full transcript. The public proxy can't reach a non-github.com
/// gist, so there it is skipped with a note rather than a pointless prompt.
fn resolve_preview(identity: &gist::Identity, requested: bool, assume_yes: bool) -> Result<bool> {
    if !requested {
        return Ok(false);
    }
    if identity.host != "github.com" {
        eprintln!(
            "note: the public preview proxy can't reach {} gists; publishing without a preview link.",
            identity.host
        );
        return Ok(false);
    }
    if assume_yes {
        return Ok(true);
    }

    println!("The preview link renders the folio through gist.githack.com, a public");
    println!("third-party proxy that fetches and caches the full transcript.");
    ask("Include the public preview link?")
}

/// Prompts a yes/no question, treating a cancellation (Esc / Ctrl-C) as "no".
fn ask(question: &str) -> Result<bool> {
    match Confirm::new(question).with_default(false).prompt() {
        Ok(answer) => Ok(answer),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn fetch(args: FetchArgs) -> Result<()> {
    let files = gist::fetch(&args.gist)?;
    let dir = args.output.unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut folio = None;
    for (name, contents) in &files {
        let path = dir.join(name);
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        println!("{}", path.display());
        if folio.is_none() && name.ends_with(".html") {
            folio = Some(path);
        }
    }

    if args.open {
        let folio = folio.context("gist holds no .html file to open")?;
        open::that(&folio).with_context(|| format!("opening {}", folio.display()))?;
    }
    Ok(())
}

/// A gist description recovered from the session's own title, falling back to
/// the session id when it has none. Collapses whitespace so a multi-line first
/// prompt (the title fallback) stays a single-line description.
fn gist_description(session: &Path, session_id: &str) -> String {
    match Folio::peek(session).title {
        Some(title) => format!(
            "Claude Code session: {}",
            title.split_whitespace().collect::<Vec<_>>().join(" ")
        ),
        None => format!("Claude Code session {session_id}"),
    }
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
