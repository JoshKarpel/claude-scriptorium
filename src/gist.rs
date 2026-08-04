//! Publishing a rendered folio to a GitHub gist through `gh`, resolving and
//! confirming the publishing account first.
//!
//! `gh gist create` has no `--hostname` flag: it publishes as whichever account
//! gh resolves for its default host, so a machine with several authenticated
//! accounts can silently push to the wrong identity. [`resolve_identity`]
//! recovers that account up front, using gh's own host precedence, so the shell
//! can confirm it before anything is published.
//!
//! **A folio is bigger than the gists API will read back.** The API truncates a
//! file over ~1 MB, answering with empty `content` and a `raw_url` to fetch
//! instead, and a folio passes that mark easily (the fonts alone are most of a
//! short one). That raw URL is the only part of the flow not served by the API,
//! and on a GitHub Enterprise instance it is served by the *web* app, which
//! authenticates by session cookie and content-negotiates: an API request lands
//! there with an API `Accept` header and is answered `406 Not Acceptable`, which
//! reads as a content-type quarrel and is really an auth failure. On github.com
//! the same URL is a plain file server that serves even a secret gist to nobody
//! in particular, which is why the raw path works there and only there.
//!
//! So nothing here reads a folio through `raw_url`. Writing goes through the API
//! (which accepts a whole folio on the way in, and truncates only on the way
//! out), and reading goes through git: a gist is a git repository, so
//! [`fetch`] clones it, which has no size limit, no content negotiation, and
//! authenticates the way every other clone on the machine does.

use std::{
    collections::HashMap,
    fmt, fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, de::IgnoredAny};
use serde_json::json;

/// This project's own viewer, served from its GitHub Pages site, used to render
/// a published github.com gist. The viewer's host never receives the transcript:
/// the reader's browser fetches the gist from the GitHub API and writes it into
/// the page (see `docs/index.html`).
pub const DEFAULT_VIEWER_BASE: &str = "https://joshkarpel.github.io/claude-scriptorium/";

/// The viewer page, embedded so `scaffold_viewer` can write a self-hostable copy.
/// It is the very file this project serves from its own Pages site, so the two
/// never drift.
const VIEWER_TEMPLATE: &str = include_str!("../docs/index.html");

/// The GitHub API the embedded viewer reads gists from by default. A scaffolded
/// enterprise viewer rewrites this to the GHES instance's API.
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// The marker every published gist's description begins with: this tool's own
/// package name. It identifies a gist as one this tool created, so the
/// management commands never touch an unrelated gist, and the session id that
/// follows lets a republish find and edit the existing gist in place rather than
/// piling up duplicates.
pub const GIST_MARKER: &str = env!("CARGO_PKG_NAME");

/// The gist description this tool stamps: the [marker](GIST_MARKER) and session
/// id, then the session's title when it has one. The marker prefix is how the
/// management commands recognise a gist as ours; the session id is how a
/// republish matches a gist back to its session.
pub fn describe(session_id: &str, title: Option<&str>) -> String {
    match title {
        Some(title) => {
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("{GIST_MARKER} {session_id}: {title}")
        }
        None => format!("{GIST_MARKER} {session_id}"),
    }
}

/// Whether a gist's description marks it as one this tool published: it begins
/// with the marker followed by a space (the session id). A `None` description,
/// or one that merely happens to contain the marker, is not ours.
fn is_ours(description: Option<&str>) -> bool {
    description.is_some_and(|description| {
        description
            .strip_prefix(GIST_MARKER)
            .is_some_and(|rest| rest.starts_with(' '))
    })
}

/// The GitHub account `gh gist create` will publish as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub login: String,
    pub host: String,
    pub token_source: String,
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} on {} (auth: {})",
            self.login, self.host, self.token_source
        )
    }
}

/// Resolves the account `gh gist create` will publish as, following gh's own
/// host precedence so the account this reports is the one that publishes.
pub fn resolve_identity() -> Result<Identity> {
    let host = resolve_host(std::env::var("GH_HOST").ok(), &authenticated_hosts()?);
    let status = gh(&[
        "auth",
        "status",
        "--active",
        "--hostname",
        &host,
        "--json",
        "hosts",
    ])?;
    parse_identity(&status, &host)
}

/// The outcome of a [`publish`]: the gist's page URL, and whether an existing
/// gist for the session was edited in place rather than a new one created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub url: String,
    pub updated: bool,
}

/// Publishes `html` for `session_id`, idempotently: if this tool already has a
/// gist for the session (matched by its `<session_id>.html` file), that gist is
/// edited in place so its URL stays stable and re-publishing doesn't pile up
/// duplicates; otherwise a new gist is created. Secret by default; `public`
/// lists it. Visibility is fixed at creation, so a republish that would change
/// it fails rather than silently ignoring the request.
pub fn publish(html: &str, session_id: &str, description: &str, public: bool) -> Result<Published> {
    let filename = format!("{session_id}.html");

    if let Some(existing) = find_ours(session_id)? {
        if existing.public != public {
            bail!(
                "session {session_id} is already published as a {} gist ({}); delete it first to change its visibility",
                visibility(existing.public),
                existing.url
            );
        }
        // The API directly rather than `gh gist edit`, which reads the file's
        // current content before writing the replacement and reaches for
        // `raw_url` to do it when the file is truncated, which every folio is.
        // A PATCH sets the content and the description in one request and never
        // reads the old bytes we are about to overwrite.
        gh_stdin(
            &[
                "api",
                "-X",
                "PATCH",
                &format!("gists/{}", existing.id),
                "--input",
                "-",
            ],
            &edit_body(&filename, description, html),
        )
        .context("editing the existing gist")?;
        return Ok(Published {
            url: existing.url,
            updated: true,
        });
    }

    let mut args = vec![
        "gist",
        "create",
        "-",
        "--filename",
        &filename,
        "--desc",
        description,
    ];
    if public {
        args.push("--public");
    }
    let url = gh_stdin(&args, html)
        .context("running gh gist create (is the GitHub CLI installed?)")?
        .trim()
        .to_owned();
    Ok(Published {
        url,
        updated: false,
    })
}

/// The body of the PATCH that republishes a session: the new description, and
/// the whole folio as the gist's one file. The API takes a folio of any size on
/// the way in; it is only reading one back that it truncates.
fn edit_body(filename: &str, description: &str, html: &str) -> String {
    json!({
        "description": description,
        "files": { filename: { "content": html } },
    })
    .to_string()
}

/// The gist this tool published for `session_id`, if any: the one among our
/// gists whose files include `<session_id>.html`.
fn find_ours(session_id: &str) -> Result<Option<PublishedGist>> {
    let filename = format!("{session_id}.html");
    Ok(list_ours()?
        .into_iter()
        .find(|gist| gist.files.iter().any(|file| file == &filename)))
}

/// Lists the gists this tool published as the active `gh` account, recognised by
/// the [marker](GIST_MARKER) their descriptions carry. `--paginate` walks every
/// page so a republish or a bulk delete sees all of them, not just the first.
pub fn list_ours() -> Result<Vec<PublishedGist>> {
    let json = gh(&["api", "gists", "--paginate"])?;
    let gists: Vec<ApiGist> = serde_json::from_str(&json).context("parsing gh api gists")?;
    Ok(gists
        .into_iter()
        .map(PublishedGist::from)
        .filter(PublishedGist::is_ours)
        .collect())
}

/// Looks up a single gist by id or URL through `gh api`, so a delete can confirm
/// it is one this tool published before removing it.
pub fn lookup(gist: &str) -> Result<PublishedGist> {
    let json = gh(&["api", &format!("gists/{}", gist_id(gist))])?;
    let gist: ApiGist = serde_json::from_str(&json).context("parsing gh api gist")?;
    Ok(gist.into())
}

/// Deletes a gist by id via `gh gist delete`. Ownership is the caller's to check
/// (see [`PublishedGist::is_ours`]); this is the mechanical removal.
pub fn delete(id: &str) -> Result<()> {
    gh(&["gist", "delete", id, "--yes"]).map(drop)
}

fn visibility(public: bool) -> &'static str {
    if public { "public" } else { "secret" }
}

/// Runs `gh` with the given arguments, piping `input` to its stdin (so the HTML
/// never lands in a temp file), and returns its stdout.
fn gh_stdin(args: &[&str], input: &str) -> Result<String> {
    let mut child = Command::new("gh")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("running gh (is the GitHub CLI installed?)")?;

    child
        .stdin
        .take()
        .context("gh stdin was not captured")?
        .write_all(input.as_bytes())
        .context("piping input to gh")?;

    let output = child.wait_with_output().context("waiting for gh")?;
    if !output.status.success() {
        bail!("gh {} failed", args.join(" "));
    }
    String::from_utf8(output.stdout).context("gh produced non-UTF-8 output")
}

/// Downloads a published gist's files by id or URL, returning each file's name
/// and contents so a folio can be viewed offline without any rendering proxy.
///
/// A gist is a git repository, and cloning it is the only way to read a folio
/// back whole: the API truncates a file this size and hands out a `raw_url` that
/// no API credential opens on an enterprise instance (see the module header).
/// The clone URL comes from the API rather than being spelled out here, so an
/// instance that keeps its gists on a subdomain is followed rather than guessed
/// at. `gh` resolves the id against its default host, the same one [`publish`]
/// targets.
pub fn fetch(gist: &str) -> Result<Vec<(String, String)>> {
    let gist = lookup(gist)?;
    let dir =
        std::env::temp_dir().join(format!("{GIST_MARKER}-{}-{}", gist.id, std::process::id()));

    // `git clone` insists on an empty target, so a directory left behind by a
    // killed run would otherwise poison every run after it.
    let _ = fs::remove_dir_all(&dir);
    let fetched = clone(&gist.clone_url, &dir).and_then(|()| worktree_files(&dir));
    let _ = fs::remove_dir_all(&dir);
    fetched
}

/// Clones `url` into `into`, shallowly, with `gh` as git's credential helper.
///
/// The helper is set for this one command rather than in the user's git config,
/// so `fetch` needs no `gh auth setup-git` and changes nothing global. The empty
/// value ahead of it clears any helper already configured: `credential.helper`
/// is a list, and a machine-wide helper answering first with a stale credential
/// would fail a clone that gh's own token would have opened.
fn clone(url: &str, into: &Path) -> Result<()> {
    let status = Command::new("git")
        .args([
            "-c",
            "credential.helper=",
            "-c",
            "credential.helper=!gh auth git-credential",
            "clone",
            "--quiet",
            "--depth",
            "1",
            url,
        ])
        .arg(into)
        .status()
        .context("running git (is git installed?)")?;
    if !status.success() {
        bail!("git clone {url} failed");
    }
    Ok(())
}

/// The files a cloned gist left in `dir`, by name, in name order. A gist's tree
/// is flat, so everything but git's own directory is one of the gist's files.
fn worktree_files(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("reading {}", dir.display()))?
            .path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("{} has no readable name", path.display()))?;
        if name == ".git" {
            continue;
        }
        let contents =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        files.push((name.to_owned(), contents));
    }
    files.sort();
    Ok(files)
}

/// The gist id from a page URL or a bare id. The API endpoints take an id, so
/// reducing a URL to its trailing id is what lets a reader paste the browser URL
/// they were handed.
fn gist_id(gist: &str) -> &str {
    gist.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(gist)
}

/// Which host `gh gist create` targets: `GH_HOST` when set, otherwise the sole
/// authenticated host, otherwise github.com. It has no `--hostname` flag, so
/// this mirrors gh's own precedence.
fn resolve_host(gh_host: Option<String>, authenticated: &[String]) -> String {
    if let Some(host) = gh_host.filter(|host| !host.is_empty()) {
        return host;
    }
    if let [only] = authenticated {
        return only.clone();
    }
    "github.com".to_owned()
}

/// The active account for `host` from `gh auth status --json hosts`.
fn parse_identity(status: &str, host: &str) -> Result<Identity> {
    let status: AuthStatus = serde_json::from_str(status).context("parsing gh auth status")?;
    let account = status
        .hosts
        .get(host)
        .into_iter()
        .flatten()
        .find(|account| account.active)
        .with_context(|| format!("gh has no active account for {host}"))?;
    Ok(Identity {
        login: account.login.clone(),
        host: account.host.clone(),
        token_source: account.token_source.clone(),
    })
}

/// The URL that renders a published gist through a viewer page. `viewer_base` is
/// the Pages site hosting the viewer (this project's by default, or a
/// self-hosted one). The viewer only needs the gist id and file name, since its
/// own script fetches the content from the GitHub API; a folio over GitHub's
/// ~1 MB API truncation limit is fetched from its raw URL by the same script.
/// Unlike a re-serving proxy, the viewer's host never receives the transcript.
pub fn preview_url(viewer_base: &str, gist_url: &str, filename: &str) -> String {
    let id = gist_id(gist_url);
    let base = viewer_base.trim_end_matches('/');
    format!("{base}/?{id}/{filename}")
}

/// A self-hostable copy of the viewer page. With no `ghes_host`, it is the
/// github.com viewer verbatim; with one, its API base is rewritten to the
/// enterprise instance (`https://<host>/api/v3`) so a viewer served from that
/// instance's Pages can read its gists.
pub fn scaffold_viewer(ghes_host: Option<&str>) -> Result<String> {
    let Some(host) = ghes_host else {
        return Ok(VIEWER_TEMPLATE.to_owned());
    };

    let api_base = format!("https://{host}/api/v3");
    let rewritten =
        VIEWER_TEMPLATE.replace(&format!("'{DEFAULT_API_BASE}'"), &format!("'{api_base}'"));
    if rewritten == VIEWER_TEMPLATE {
        bail!("viewer template no longer contains the API base to rewrite for a GHES host");
    }
    Ok(rewritten)
}

/// The README written beside a scaffolded viewer: how to deploy it, how to point
/// `publish` at it once, and, for an enterprise host, what the viewer cannot do
/// there and what to reach for instead.
///
/// `viewer_base_env` is the environment variable that defaults the preview base,
/// named here rather than assumed, so the one place that owns it stays the
/// shell.
pub fn viewer_readme(host: Option<&str>, viewer_base_env: &str) -> String {
    let tool = env!("CARGO_PKG_NAME");
    let reads_from = host.unwrap_or("github.com");
    let ghes_note = match host {
        Some(host) => format!(
            "\n## On {host}\n\n\
             This viewer reads gists from `{host}` (its `/api/v3` endpoint), so it must be \
             served from that instance's Pages and the instance must enable Pages and allow \
             cross-origin API requests from the Pages origin.\n\n\
             Two things an enterprise instance does differently will stop it, and both are \
             worth checking before you deploy:\n\n\
             - **The page reads the API as nobody.** It carries no token, and a browser sends \
             no cookie across origins, so an instance in private mode answers the fetch with a \
             redirect to its sign-in page rather than the gist. There is nowhere for a static \
             page to get a credential from.\n\
             - **A folio is usually over the API's ~1 MB limit**, and the API then answers with \
             a `raw_url` instead of the content. On `github.com` that URL is an open file \
             server; on an enterprise instance it is the web app, which wants a session cookie \
             the fetch cannot send.\n\n\
             Where either holds, `{tool} fetch <gist> --open` is the way to read a folio: it \
             clones the gist with your own git credentials and opens the file locally, with no \
             viewer in the loop at all.\n"
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
         Point `publish` at it once, by setting the base in your shell profile:\n\n\
         ```\n\
         export {viewer_base_env}=https://<owner>.github.io/<repo>/\n\
         ```\n\n\
         Or per publish, with `{tool} publish --preview-base <url>`.\n\n\
         Vendored from GistHost (MIT); see the license header in `index.html`.\n"
    )
}

/// The hosts gh has an authenticated account for, in gh's own order.
fn authenticated_hosts() -> Result<Vec<String>> {
    let output = gh(&[
        "auth",
        "status",
        "--json",
        "hosts",
        "--jq",
        ".hosts | keys[]",
    ])?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Runs `gh` with the given arguments and returns its stdout, letting gh's own
/// diagnostics through on stderr.
fn gh(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .context("running gh (is the GitHub CLI installed?)")?;
    if !output.status.success() {
        bail!("gh {} failed", args.join(" "));
    }
    String::from_utf8(output.stdout).context("gh produced non-UTF-8 output")
}

/// A gist as `gh api gists` reports it, kept minimal to what the management
/// commands need.
#[derive(Deserialize)]
struct ApiGist {
    id: String,
    description: Option<String>,
    public: bool,
    html_url: String,
    git_pull_url: String,
    files: HashMap<String, IgnoredAny>,
}

/// A gist recovered from `gh api`, refined to what the shell shows and acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedGist {
    pub id: String,
    pub description: Option<String>,
    pub public: bool,
    pub url: String,
    /// Where to clone the gist from, as the instance itself reports it, which is
    /// how [`fetch`] reads a folio back whole.
    pub clone_url: String,
    pub files: Vec<String>,
}

impl PublishedGist {
    /// Whether this gist was published by this tool (its description carries the
    /// [marker](GIST_MARKER)), so a delete can refuse to touch anything else.
    pub fn is_ours(&self) -> bool {
        is_ours(self.description.as_deref())
    }
}

impl From<ApiGist> for PublishedGist {
    fn from(gist: ApiGist) -> Self {
        let mut files: Vec<String> = gist.files.into_keys().collect();
        files.sort();
        PublishedGist {
            id: gist.id,
            description: gist.description,
            public: gist.public,
            url: gist.html_url,
            clone_url: gist.git_pull_url,
            files,
        }
    }
}

#[derive(Deserialize)]
struct AuthStatus {
    hosts: HashMap<String, Vec<Account>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    login: String,
    host: String,
    token_source: String,
    active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_stamps_the_marker_session_id_and_title() {
        let description = describe("abc123", Some("Investigate the missing panels"));
        assert_eq!(
            description,
            "claude-scriptorium abc123: Investigate the missing panels"
        );
    }

    #[test]
    fn describe_collapses_whitespace_in_a_multiline_title() {
        let description = describe("abc123", Some("first line\n\n  second   line"));
        assert_eq!(
            description,
            "claude-scriptorium abc123: first line second line"
        );
    }

    #[test]
    fn describe_omits_the_title_when_the_session_has_none() {
        assert_eq!(describe("abc123", None), "claude-scriptorium abc123");
    }

    #[test]
    fn is_ours_accepts_a_description_stamped_by_this_tool() {
        assert!(is_ours(Some("claude-scriptorium abc123: a title")));
        assert!(is_ours(Some("claude-scriptorium abc123")));
    }

    #[test]
    fn is_ours_rejects_foreign_and_missing_descriptions() {
        assert!(!is_ours(Some("Claude Code session: a title")));
        assert!(!is_ours(Some("claude-scriptorium-fork abc123")));
        assert!(!is_ours(Some("claude-scriptorium")));
        assert!(!is_ours(None));
    }

    #[test]
    fn published_gist_recovers_id_visibility_url_and_file_names() {
        let json = r#"{
            "id": "7f15",
            "description": "claude-scriptorium abc123: a title",
            "public": true,
            "html_url": "https://gist.github.com/scribe/7f15",
            "git_pull_url": "https://gist.github.com/7f15.git",
            "files": {"abc123.html": {"filename": "abc123.html"}}
        }"#;
        let gist: PublishedGist = serde_json::from_str::<ApiGist>(json).unwrap().into();

        assert_eq!(gist.id, "7f15");
        assert!(gist.public);
        assert_eq!(gist.url, "https://gist.github.com/scribe/7f15");
        assert_eq!(gist.clone_url, "https://gist.github.com/7f15.git");
        assert_eq!(gist.files, vec!["abc123.html".to_owned()]);
        assert!(gist.is_ours());
    }

    #[test]
    fn edit_body_carries_the_description_and_the_whole_folio() {
        let body: serde_json::Value =
            serde_json::from_str(&edit_body("abc123.html", "a description", "<p>folio</p>"))
                .unwrap();

        assert_eq!(body["description"], "a description");
        assert_eq!(body["files"]["abc123.html"]["content"], "<p>folio</p>");
    }

    #[test]
    fn a_github_com_viewer_readme_names_the_env_var_and_raises_no_enterprise_caveat() {
        let readme = viewer_readme(None, "CLAUDE_SCRIPTORIUM_VIEWER_BASE");

        assert!(readme.contains("export CLAUDE_SCRIPTORIUM_VIEWER_BASE=https://"));
        assert!(!readme.contains("private mode"));
    }

    #[test]
    fn an_enterprise_viewer_readme_names_the_host_and_points_at_fetch_instead() {
        let readme = viewer_readme(Some("ghe.example.com"), "CLAUDE_SCRIPTORIUM_VIEWER_BASE");

        assert!(readme.contains("gists from `ghe.example.com`"));
        assert!(readme.contains("private mode"));
        assert!(readme.contains("claude-scriptorium fetch <gist> --open"));
        assert!(readme.contains("export CLAUDE_SCRIPTORIUM_VIEWER_BASE=https://"));
    }

    #[test]
    fn published_gist_is_not_ours_without_the_marker() {
        let json = r#"{
            "id": "7f15",
            "description": "some unrelated gist",
            "public": false,
            "html_url": "https://gist.github.com/scribe/7f15",
            "git_pull_url": "https://gist.github.com/7f15.git",
            "files": {"notes.txt": {"filename": "notes.txt"}}
        }"#;
        let gist: PublishedGist = serde_json::from_str::<ApiGist>(json).unwrap().into();

        assert!(!gist.is_ours());
    }

    #[test]
    fn gh_host_env_wins_over_authenticated_hosts() {
        let host = resolve_host(
            Some("ghe.example.com".to_owned()),
            &["github.com".to_owned()],
        );
        assert_eq!(host, "ghe.example.com");
    }

    #[test]
    fn empty_gh_host_falls_through_to_the_sole_authenticated_host() {
        let host = resolve_host(Some(String::new()), &["ghe.example.com".to_owned()]);
        assert_eq!(host, "ghe.example.com");
    }

    #[test]
    fn several_authenticated_hosts_default_to_github_com() {
        let host = resolve_host(
            None,
            &["ghe.example.com".to_owned(), "github.com".to_owned()],
        );
        assert_eq!(host, "github.com");
    }

    #[test]
    fn no_authenticated_hosts_default_to_github_com() {
        assert_eq!(resolve_host(None, &[]), "github.com");
    }

    #[test]
    fn parse_identity_picks_the_active_account_for_the_host() {
        let status = r#"{"hosts":{"github.com":[
            {"login":"other","host":"github.com","tokenSource":"keyring","active":false},
            {"login":"scribe","host":"github.com","tokenSource":"/etc/gh/hosts.yml","active":true}
        ]}}"#;

        let identity = parse_identity(status, "github.com").unwrap();

        assert_eq!(identity.login, "scribe");
        assert_eq!(identity.host, "github.com");
        assert_eq!(identity.token_source, "/etc/gh/hosts.yml");
    }

    #[test]
    fn parse_identity_fails_when_the_host_has_no_active_account() {
        let status = r#"{"hosts":{"github.com":[
            {"login":"scribe","host":"github.com","tokenSource":"keyring","active":false}
        ]}}"#;

        assert!(parse_identity(status, "github.com").is_err());
    }

    #[test]
    fn preview_url_addresses_the_gist_through_the_viewer_base() {
        let preview = preview_url(
            "https://joshkarpel.github.io/claude-scriptorium/",
            "https://gist.github.com/scribe/abc123",
            "session-7.html",
        );
        assert_eq!(
            preview,
            "https://joshkarpel.github.io/claude-scriptorium/?abc123/session-7.html"
        );
    }

    #[test]
    fn preview_url_tolerates_a_viewer_base_without_a_trailing_slash() {
        let preview = preview_url(
            "https://viewer.example.com",
            "https://gist.github.com/scribe/abc123",
            "session-7.html",
        );
        assert_eq!(preview, "https://viewer.example.com/?abc123/session-7.html");
    }

    #[test]
    fn gist_id_reduces_a_page_url_to_its_trailing_id() {
        assert_eq!(gist_id("https://gist.github.com/scribe/abc123"), "abc123");
        assert_eq!(gist_id("https://gist.github.com/scribe/abc123/"), "abc123");
        assert_eq!(gist_id("abc123"), "abc123");
    }

    #[test]
    fn scaffold_viewer_is_the_template_verbatim_for_github_com() {
        assert_eq!(scaffold_viewer(None).unwrap(), VIEWER_TEMPLATE);
    }

    #[test]
    fn scaffold_viewer_rewrites_the_api_base_for_a_ghes_host() {
        let viewer = scaffold_viewer(Some("ghe.example.com")).unwrap();
        assert!(viewer.contains("'https://ghe.example.com/api/v3'"));
        assert!(!viewer.contains("'https://api.github.com'"));
    }
}
