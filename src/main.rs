use std::{
    collections::BTreeMap,
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use claude_scriptorium::{
    discovery, gist, picker, render,
    render::{Colophon, Delivery, Labour, Scribe},
    serve,
    transcript::Folio,
};
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use inquire::{Confirm, InquireError};
use jiff::{Timestamp, tz::TimeZone};

/// Environment variable holding a default preview viewer base, so a machine
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
    /// List the gists this tool has published.
    Gists,
    /// Delete a gist this tool published (by id/URL, or all of them).
    Delete(DeleteArgs),
    /// Download a published gist's files to view a folio offline.
    Fetch(FetchArgs),
    /// Scaffold a self-hostable folio-viewer site to serve from GitHub Pages
    /// (including a GHES instance).
    ScaffoldViewer(ScaffoldViewerArgs),
}

/// How a folio should carry its fonts. Shared by every subcommand that writes
/// one, so a folio means the same thing however it was produced.
#[derive(Args)]
struct Faces {
    /// Embed the whole upstream fonts rather than the cut ones, adding ~2 MB.
    /// A folio only needs this when it will later gain text the session did not
    /// have; a folio that already sets such a character switches on its own.
    #[arg(long)]
    whole_fonts: bool,
}

impl Faces {
    fn choice(&self) -> render::Fonts {
        if self.whole_fonts {
            render::Fonts::Whole
        } else {
            render::Fonts::Fitted
        }
    }
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

    #[command(flatten)]
    faces: Faces,

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

    #[command(flatten)]
    faces: Faces,

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

    #[command(flatten)]
    faces: Faces,

    /// List the gist publicly instead of keeping it secret (the default).
    #[arg(long)]
    public: bool,

    /// Viewer base URL for the preview link, e.g. a self-hosted or GHES viewer
    /// from `scaffold-viewer`. Falls back to $CLAUDE_SCRIPTORIUM_VIEWER_BASE,
    /// then to this project's viewer for github.com gists.
    #[arg(long, value_name = "URL")]
    preview_base: Option<String>,

    /// Skip all confirmation prompts (for non-interactive use).
    #[arg(long)]
    yes: bool,

    /// Open the published folio in the default browser (the preview link when
    /// there is a viewer for the gist's host, otherwise the gist page).
    #[arg(long)]
    open: bool,
}

#[derive(Args)]
struct DeleteArgs {
    /// Gist id or URL to delete. Omit together with `--all` to delete every
    /// gist this tool has published.
    gist: Option<String>,

    /// Delete every gist this tool has published, after listing and confirming
    /// them.
    #[arg(long)]
    all: bool,

    /// Skip the confirmation prompt (for non-interactive use).
    #[arg(long)]
    yes: bool,
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

    /// GitHub host the viewer will read gists from, e.g. `ghe.example.com`.
    /// Defaults to the host `publish` targets, as `gh` resolves it.
    #[arg(long, value_name = "HOST")]
    host: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(args) => render(args),
        Command::Serve(args) => serve(args),
        Command::Publish(args) => publish(args),
        Command::Gists => gists(),
        Command::Delete(args) => delete(args),
        Command::Fetch(args) => fetch(args),
        Command::ScaffoldViewer(args) => scaffold_viewer(args),
    }
}

fn render(args: RenderArgs) -> Result<()> {
    let session = resolve_session(args.selection)?;
    let folio = Folio::read(&session)?;
    let output = output_path(args.output, &folio)?;

    let highlighter = highlighter();
    let scribe = Scribe::new(
        &highlighter,
        TimeZone::system(),
        args.faces.choice(),
        Delivery::Static,
    );
    let (html, labour, reached) = inscribe(&scribe, &folio);

    fs::write(&output, &html).with_context(|| format!("writing {}", output.display()))?;
    println!("{}", output.display());
    report(&html, &labour, &reached);

    if args.open {
        open::that(&output).with_context(|| format!("opening {}", output.display()))?;
    }
    Ok(())
}

fn serve(args: ServeArgs) -> Result<()> {
    let session = resolve_session(args.selection)?;
    let highlighter = highlighter();
    // The one folio that can gain a message under its reader: this server
    // re-reads the session and re-renders on every load.
    let scribe = Scribe::new(
        &highlighter,
        TimeZone::system(),
        args.faces.choice(),
        Delivery::Served,
    );

    serve::run(args.port, &session, args.open, || {
        let folio = Folio::read(&session)?;
        let (html, labour, reached) = inscribe(&scribe, &folio);
        report(&html, &labour, &reached);
        Ok(html)
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
    let viewer_base = resolve_viewer(&identity, base_override);

    let highlighter = highlighter();
    let scribe = Scribe::new(
        &highlighter,
        TimeZone::system(),
        args.faces.choice(),
        Delivery::Static,
    );
    let (html, labour, reached) = inscribe(&scribe, &folio);
    report(&html, &labour, &reached);

    let session_id = folio.session_id();
    let filename = format!("{session_id}.html");
    let description = gist::describe(session_id, Folio::peek(&session).title.as_deref());
    let published = gist::publish(&html, session_id, &description, args.public)?;

    if published.updated {
        println!("Updated the gist already published for this session");
    }
    let gist_url = published.url;
    println!("{gist_url}");

    // The gist page shows a folio's HTML source, never the folio: GitHub serves
    // gist content as text/plain with `nosniff`, on the raw URL as much as the
    // page, so nothing on GitHub renders it at any size. Reading a published
    // folio therefore always takes one of the two routes below.
    //
    // Prose unindented, a blank line, then the thing to click or run indented
    // under it, so both routes read the same way.
    let preview = viewer_base.map(|base| gist::preview_url(&base, &gist_url, &filename));
    if let Some(preview) = &preview {
        println!();
        println!("The gist page shows this folio's source, not the folio. A viewer page");
        println!("renders it in a browser, fetching the gist straight from GitHub in the");
        println!("reader's own browser, so the viewer's host never sees the transcript:");
        println!();
        println!("  {preview}");
    }

    println!();
    println!("Anyone can view it locally, with no proxy, by running:");
    println!();
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
    // What the visibility actually means rides on the prompt as its help line,
    // so the caveat is in front of the reader at the moment they answer rather
    // than scrolled above it.
    let caveat = if public {
        format!(
            "A public gist is listed on {} and readable by anyone.",
            identity.host
        )
    } else {
        format!(
            "A secret gist is unlisted, but anyone with access to {} and the URL can read it.",
            identity.host
        )
    };

    println!("Publishing this session as a {visibility} gist, as {identity}.");
    if !ask(&format!("Publish this {visibility} gist?"), Some(&caveat))? {
        bail!("aborted");
    }
    Ok(())
}

/// Chooses the viewer base for the preview link, or `None` when there is no
/// viewer for the gist's host. An explicit base (`--preview-base` or the env
/// var, resolved by the caller) wins; a github.com gist falls back to this
/// project's own viewer; any other host (a GHES instance) has no built-in
/// viewer, so the link is skipped with a pointer to `scaffold-viewer`. Printing
/// the link is harmless (the caller notes that only a reader's browser, not the
/// viewer's host, ever fetches the transcript), so this only picks the base and
/// never prompts.
fn resolve_viewer(identity: &gist::Identity, base_override: Option<String>) -> Option<String> {
    match base_override {
        Some(base) => Some(base),
        None if identity.host == "github.com" => Some(gist::DEFAULT_VIEWER_BASE.to_owned()),
        None => {
            eprintln!(
                "Note: no built-in viewer for {} gists; scaffold one with `{} scaffold-viewer` and set {VIEWER_BASE_ENV}. Publishing without a preview link.",
                identity.host,
                env!("CARGO_PKG_NAME")
            );
            None
        }
    }
}

/// Prompts a yes/no question, treating a cancellation (Esc / Ctrl-C) as "no".
/// `help` rides along under the prompt, for a caveat the answer turns on.
fn ask(question: &str, help: Option<&str>) -> Result<bool> {
    let mut prompt = Confirm::new(question).with_default(false);
    if let Some(help) = help {
        prompt = prompt.with_help_message(help);
    }
    match prompt.prompt() {
        Ok(answer) => Ok(answer),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Confirms a destructive action, refusing rather than proceeding when there is
/// no terminal to prompt at and `--yes` was not given.
fn require_confirmation(question: &str, assume_yes: bool) -> Result<()> {
    if assume_yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("refusing without confirmation: pass --yes");
    }
    if !ask(question, None)? {
        bail!("aborted");
    }
    Ok(())
}

fn gists() -> Result<()> {
    let identity = gist::resolve_identity()?;
    let ours = gist::list_ours()?;
    if ours.is_empty() {
        println!(
            "No gists published by {} as {identity}",
            env!("CARGO_PKG_NAME")
        );
        return Ok(());
    }
    for gist in &ours {
        print_gist(gist);
    }
    Ok(())
}

fn delete(args: DeleteArgs) -> Result<()> {
    let identity = gist::resolve_identity()?;
    if args.all {
        if args.gist.is_some() {
            bail!("pass a gist id/URL or --all, not both");
        }
        return delete_all(&identity, args.yes);
    }

    let gist = args
        .gist
        .context("give a gist id or URL to delete, or --all to delete every published gist")?;
    let found = gist::lookup(&gist)?;
    if !found.is_ours() {
        bail!(
            "{} was not published by {} and will not be deleted",
            found.id,
            env!("CARGO_PKG_NAME")
        );
    }

    println!("Deleting this gist, published as {identity}:");
    print_gist(&found);
    require_confirmation("Delete it?", args.yes)?;
    gist::delete(&found.id)?;
    println!("Deleted {}", found.id);
    Ok(())
}

/// Deletes every gist this tool published, listing them and confirming as a
/// batch so a bulk delete is never a blind one.
fn delete_all(identity: &gist::Identity, assume_yes: bool) -> Result<()> {
    let ours = gist::list_ours()?;
    if ours.is_empty() {
        println!(
            "No gists published by {} as {identity}",
            env!("CARGO_PKG_NAME")
        );
        return Ok(());
    }

    println!(
        "Deleting these {} gists, published as {identity}:",
        ours.len()
    );
    for gist in &ours {
        print_gist(gist);
    }
    require_confirmation("Delete all of them?", assume_yes)?;
    for gist in &ours {
        gist::delete(&gist.id)?;
        println!("Deleted {}", gist.id);
    }
    Ok(())
}

fn print_gist(gist: &gist::PublishedGist) {
    let visibility = if gist.public { "public" } else { "secret" };
    println!("  {}  ({visibility})", gist.url);
    if let Some(description) = &gist.description {
        println!("    {description}");
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
    let host = scaffold_host(args.host)?;
    let viewer = gist::scaffold_viewer(host.as_deref())?;

    fs::create_dir_all(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    let index = args.output.join("index.html");
    fs::write(&index, viewer).with_context(|| format!("writing {}", index.display()))?;
    let readme = args.output.join("README.md");
    fs::write(
        &readme,
        gist::viewer_readme(host.as_deref(), VIEWER_BASE_ENV),
    )
    .with_context(|| format!("writing {}", readme.display()))?;

    git_init(&args.output);

    println!("{}", index.display());
    println!("{}", readme.display());
    println!(
        "Reads gists from {}.",
        host.as_deref().unwrap_or("github.com")
    );
    println!(
        "Next: push {} to GitHub, enable Pages (Deploy from a branch, / root), then set {VIEWER_BASE_ENV} to your Pages URL",
        args.output.display()
    );
    Ok(())
}

/// The host a scaffolded viewer reads gists from: the one given, otherwise the
/// one `gh` publishes to, so a viewer scaffolded on a work machine points at
/// that machine's instance without being told. `None` means github.com, which
/// the viewer template already targets and so needs no rewrite.
///
/// Asking `gh` is what makes the default right rather than merely present, so a
/// gh that can't answer is an error naming the flag rather than a silent
/// github.com viewer for an enterprise instance.
fn scaffold_host(given: Option<String>) -> Result<Option<String>> {
    let host = match given {
        Some(host) => host,
        None => {
            gist::resolve_identity()
                .context("could not resolve the GitHub host from gh; pass --host")?
                .host
        }
    };
    Ok((host != "github.com").then_some(host))
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
            "Note: could not run `git init` in {}; do it yourself",
            dir.display()
        );
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

/// Sets a folio and records what setting it cost, filling both figures into the
/// plaque's placeholders. The time is the render alone, so it means the same
/// thing under every subcommand; the size is of the markup the figures are
/// substituted into, which is the whole folio bar the substitution itself.
fn inscribe(scribe: &Scribe, folio: &Folio) -> (String, Labour, BTreeMap<char, usize>) {
    let started = Instant::now();
    let (markup, reached) = scribe.folio(folio, &colophon());
    let markup = markup.into_string();
    let labour = Labour {
        took: started.elapsed(),
        bytes: markup.len(),
    };
    (render::inscribe(markup, &labour), labour, reached)
}

/// Reports what a render cost, using the same formatting helpers as the folio's own plaque.
/// The size is the finished document's, so it is exact where the plaque's is
/// the pre-substitution measure it could take of itself. It goes to stderr so
/// stdout stays the folio's path alone, for a script to consume.
///
/// A folio driven onto the whole faces says so and names what drove it, since
/// it is otherwise a silent five-fold jump in the file a reader downloads.
fn report(html: &str, labour: &Labour, reached: &BTreeMap<char, usize>) {
    eprintln!(
        "{} in {}",
        render::size(html.len()),
        render::elapsed(labour.took)
    );
    if !reached.is_empty() {
        eprintln!("Note: embedding the whole fonts: {}", named(reached));
    }
}

/// Names the characters that drove a folio onto the whole faces, most frequent
/// first, keeping the note to one line however long the tail is.
fn named(reached: &BTreeMap<char, usize>) -> String {
    const SHOWN: usize = 5;

    let mut by_frequency: Vec<(&char, &usize)> = reached.iter().collect();
    by_frequency.sort_by_key(|(character, count)| (std::cmp::Reverse(**count), **character));

    let named: Vec<String> = by_frequency
        .iter()
        .take(SHOWN)
        .map(|(character, count)| format!("{character} (U+{:04X}) ×{count}", **character as u32))
        .collect();
    let rest = by_frequency.len().saturating_sub(SHOWN);
    let tail = if rest > 0 {
        format!(", and {rest} more")
    } else {
        String::new()
    };
    format!(
        "this session sets {} the cut ones drop: {}{tail}",
        match by_frequency.len() {
            1 => "a character".to_owned(),
            count => format!("{count} characters"),
        },
        named.join(", "),
    )
}

fn colophon() -> Colophon {
    Colophon {
        generated: Timestamp::now(),
        tool: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        home: env!("CARGO_PKG_REPOSITORY"),
    }
}
