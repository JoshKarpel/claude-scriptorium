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
use serde::Deserialize;

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

/// Publishes `html` as a gist named `filename`, piping it to `gh gist create` so
/// the HTML never lands in a temp file, and returns the gist's page URL. Secret
/// by default; `public` lists it.
pub fn publish(html: &str, filename: &str, description: &str, public: bool) -> Result<String> {
    let mut args = vec![
        "gist",
        "create",
        "-",
        "--filename",
        filename,
        "--desc",
        description,
    ];
    if public {
        args.push("--public");
    }

    let mut child = Command::new("gh")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("running gh gist create (is the GitHub CLI installed?)")?;

    child
        .stdin
        .take()
        .context("gh gist create stdin was not captured")?
        .write_all(html.as_bytes())
        .context("piping html to gh gist create")?;

    let output = child
        .wait_with_output()
        .context("waiting for gh gist create")?;
    if !output.status.success() {
        bail!("gh gist create failed");
    }

    let gist_url = String::from_utf8(output.stdout)
        .context("gh gist create produced non-UTF-8 output")?
        .trim()
        .to_owned();

    Ok(gist_url)
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

/// The public-proxy URL that renders a gist's HTML in a browser, or `None` for a
/// gist not on github.com (an enterprise host the public proxy can't reach).
/// githack re-serves the raw gist content as `text/html` (GitHub's own raw
/// endpoint hands it back as `text/plain`, which browsers show as source), so a
/// shared folio renders instead of displaying its markup. Routing content
/// through a third party is why callers opt into this rather than getting it by
/// default. The owner and id come straight from the gist page URL.
pub fn preview_url(gist_url: &str, filename: &str) -> Option<String> {
    let (login, id) = gist_url
        .strip_prefix("https://gist.github.com/")?
        .split_once('/')?;
    Some(format!(
        "https://gist.githack.com/{login}/{id}/raw/{filename}"
    ))
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
    fn preview_url_points_at_githack_for_the_gist_file() {
        let preview = preview_url("https://gist.github.com/scribe/abc123", "session-7.html");
        assert_eq!(
            preview.as_deref(),
            Some("https://gist.githack.com/scribe/abc123/raw/session-7.html")
        );
    }

    #[test]
    fn gist_id_reduces_a_page_url_to_its_trailing_id() {
        assert_eq!(gist_id("https://gist.github.com/scribe/abc123"), "abc123");
        assert_eq!(gist_id("https://gist.github.com/scribe/abc123/"), "abc123");
        assert_eq!(gist_id("abc123"), "abc123");
    }

    #[test]
    fn preview_url_is_absent_for_a_non_github_gist_host() {
        assert_eq!(
            preview_url(
                "https://gist.ghe.example.com/scribe/abc123",
                "session-7.html"
            ),
            None
        );
    }
}
