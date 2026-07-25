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

/// Environment variable holding a default `--preview` viewer base, so a machine
/// that always publishes to one viewer (a work laptop on a GHES instance) can
/// set it once instead of passing `--preview-base` every time.
const VIEWER_BASE_ENV: &str = "CLAUDE_SCRIPTORIUM_VIEWER_BASE";

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
    /// Scaffold a self-hostable folio-viewer site to serve from GitHub Pages
    /// (including a GHES instance).
    ScaffoldViewer(ScaffoldViewerArgs),
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

    /// Also print a preview link that renders the folio in a browser through a
    /// viewer page (this project's Pages site by default). Off by default; the
    /// reader's browser fetches the gist from GitHub, so the viewer host never
    /// sees the transcript.
    #[arg(long)]
    preview: bool,

    /// Viewer base URL for `--preview`, e.g. a self-hosted or GHES viewer from
    /// `scaffold-viewer`. Falls back to $CLAUDE_SCRIPTORIUM_VIEWER_BASE, then to
    /// this project's viewer for github.com gists.
    #[arg(long, value_name = "URL")]
    preview_base: Option<String>,

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

#[derive(Args)]
struct ScaffoldViewerArgs {
    /// Directory to create the viewer repo in. Missing parents are created.
    output: PathBuf,

    /// GitHub Enterprise host the viewer will read gists from, e.g.
    /// `ghe.example.com`. Defaults to github.com.
    #[arg(long, value_name = "HOST")]
    host: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(args) => render(args),
        Command::Serve(args) => serve(args),
        Command::Publish(args) => publish(args),
        Command::Fetch(args) => fetch(args),
        Command::ScaffoldViewer(args) => scaffold_viewer(args),
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
    let base_override = args.preview_base.or_else(|| {
        std::env::var(VIEWER_BASE_ENV)
            .ok()
            .filter(|value| !value.is_empty())
    });
    let viewer_base = resolve_viewer(&identity, args.preview, base_override, args.yes)?;

    let highlighter = highlighter();
    let scribe = Scribe::new(&highlighter, TimeZone::system());
    let html = scribe.folio(&folio, &colophon()).into_string();

    let filename = format!("{}.html", folio.session_id());
    let description = gist_description(&session, folio.session_id());
    let gist_url = gist::publish(&html, &filename, &description, args.public)?;

    println!("{gist_url}");
    let preview = viewer_base.map(|base| gist::preview_url(&base, &gist_url, &filename));
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

/// Chooses the viewer base for the preview link, or `None` to publish without
/// one. Requested with `--preview`, an explicit base (`--preview-base` or the
/// env var, resolved by the caller) wins; otherwise a github.com gist falls back
/// to this project's own viewer, and any other host (a GHES instance) has no
/// built-in viewer, so it is skipped with a pointer to `scaffold-viewer`. The
/// choice is confirmed, since the link renders the folio through a viewer page
/// even though its content stays between GitHub and the reader's browser.
fn resolve_viewer(
    identity: &gist::Identity,
    requested: bool,
    base_override: Option<String>,
    assume_yes: bool,
) -> Result<Option<String>> {
    if !requested {
        return Ok(None);
    }

    let base = match base_override {
        Some(base) => base,
        None if identity.host == "github.com" => gist::DEFAULT_VIEWER_BASE.to_owned(),
        None => {
            eprintln!(
                "note: no built-in viewer for {} gists; scaffold one with `{} scaffold-viewer` and pass --preview-base. Publishing without a preview link.",
                identity.host,
                env!("CARGO_PKG_NAME")
            );
            return Ok(None);
        }
    };

    if assume_yes {
        return Ok(Some(base));
    }

    println!("The preview link renders the folio through a viewer page at {base}.");
    println!(
        "The reader's browser fetches the gist straight from GitHub; the viewer's host never sees it."
    );
    Ok(ask("Include the preview link?")?.then_some(base))
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

fn scaffold_viewer(args: ScaffoldViewerArgs) -> Result<()> {
    let viewer = gist::scaffold_viewer(args.host.as_deref())?;

    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    let index = args.output.join("index.html");
    fs::write(&index, viewer).with_context(|| format!("writing {}", index.display()))?;
    let readme = args.output.join("README.md");
    fs::write(&readme, viewer_readme(args.host.as_deref()))
        .with_context(|| format!("writing {}", readme.display()))?;

    git_init(&args.output);

    println!("{}", index.display());
    println!("{}", readme.display());
    println!(
        "next: push {} to GitHub, enable Pages (Deploy from a branch, / root), then publish with --preview-base <your Pages URL>",
        args.output.display()
    );
    Ok(())
}

/// Initializes a git repo in `dir`, since the scaffold is meant to be pushed to
/// GitHub Pages. A missing or failing git is a note, not a failure: the viewer
/// files are already written and the user can run `git init` themselves.
fn git_init(dir: &Path) {
    let ran = std::process::Command::new("git")
        .arg("init")
        .arg(dir)
        .stdout(std::process::Stdio::null())
        .status();
    if !matches!(ran, Ok(status) if status.success()) {
        eprintln!(
            "note: could not run `git init` in {}; do it yourself",
            dir.display()
        );
    }
}

/// The README written alongside a scaffolded viewer: how to deploy it and point
/// `publish --preview-base` at it, with the GHES caveats spelled out.
fn viewer_readme(host: Option<&str>) -> String {
    let reads_from = host.unwrap_or("github.com");
    let ghes_note = match host {
        Some(host) => format!(
            "\nThis viewer reads gists from `{host}` (its `/api/v3` endpoint). It must be \
             served from that instance's Pages, and the instance must allow cross-origin \
             requests from the Pages origin to its API and enable GitHub Pages.\n"
        ),
        None => String::new(),
    };

    format!(
        "# Folio viewer\n\n\
         A self-hostable page that renders a Claude Code folio published as a gist on \
         `{reads_from}`. The reader's browser fetches the gist from the GitHub API and \
         writes it into the page, so this site's host never receives the transcript.\n\
         {ghes_note}\n\
         ## Deploy\n\n\
         1. Push this directory to a repository.\n\
         2. Enable GitHub Pages for it: Settings, Pages, Deploy from a branch, your \
         branch, `/` (root).\n\
         3. The viewer is then served at your Pages URL, e.g. \
         `https://<owner>.github.io/<repo>/`.\n\n\
         ## Use\n\n\
         Point `publish` at it:\n\n\
         ```\n\
         {} publish --preview --preview-base https://<owner>.github.io/<repo>/\n\
         ```\n\n\
         Vendored from GistHost (MIT); see the license header in `index.html`.\n",
        env!("CARGO_PKG_NAME")
    )
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
