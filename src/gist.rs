//! Publishing a rendered folio to a GitHub gist through `gh`, resolving and
//! confirming the publishing account first.
//!
//! `gh gist create` has no `--hostname` flag: it publishes as whichever account
//! gh resolves for its default host, so a machine with several authenticated
//! accounts can silently push to the wrong identity. [`resolve_identity`]
//! recovers that account up front, using gh's own host precedence, so the shell
//! can confirm it before anything is published.

use std::{
    collections::HashMap,
    fmt,
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, de::IgnoredAny};

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
        // Content and description in one call, and the trailing `-` is what
        // makes it safe: `gh gist edit --desc` does not return once it has set
        // the description, it falls through to the file-edit loop, and with no
        // source file that loop opens $EDITOR. A second, description-only call
        // would therefore launch an editor against a piped stdin rather than
        // update anything. One call sets both in a single request.
        gh_stdin(
            &[
                "gist",
                "edit",
                &existing.id,
                "--filename",
                &filename,
                "--desc",
                description,
                "-",
            ],
            html,
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

/// Downloads a published gist's raw files by id or URL via `gh gist view`,
/// returning each file's name and contents so a folio can be viewed offline
/// without any rendering proxy. `gh` resolves the id against its default host,
/// the same one [`publish`] targets.
pub fn fetch(gist: &str) -> Result<Vec<(String, String)>> {
    let id = gist_id(gist);
    let files = gh(&["gist", "view", id, "--files"])?;

    let mut fetched = Vec::new();
    for name in files.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let contents = gh(&["gist", "view", id, "--raw", "--filename", name])?;
        fetched.push((name.to_owned(), contents));
    }
    Ok(fetched)
}

/// The gist id from a page URL or a bare id: `gh gist view` accepts either, but
/// reducing a URL to its trailing id keeps a copied browser URL working too.
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
    files: HashMap<String, IgnoredAny>,
}

/// A gist recovered from `gh api`, refined to what the shell shows and acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedGist {
    pub id: String,
    pub description: Option<String>,
    pub public: bool,
    pub url: String,
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
            "files": {"abc123.html": {"filename": "abc123.html"}}
        }"#;
        let gist: PublishedGist = serde_json::from_str::<ApiGist>(json).unwrap().into();

        assert_eq!(gist.id, "7f15");
        assert!(gist.public);
        assert_eq!(gist.url, "https://gist.github.com/scribe/7f15");
        assert_eq!(gist.files, vec!["abc123.html".to_owned()]);
        assert!(gist.is_ours());
    }

    #[test]
    fn published_gist_is_not_ours_without_the_marker() {
        let json = r#"{
            "id": "7f15",
            "description": "some unrelated gist",
            "public": false,
            "html_url": "https://gist.github.com/scribe/7f15",
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
