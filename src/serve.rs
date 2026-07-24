//! A tiny development server that serves a folio with live reload.
//!
//! This is dev-loop tooling, not the artifact: the written `.html` stays
//! self-contained and script-free. Only the served response carries the reload
//! snippet, injected before `</body>`. The page reloads when the server's
//! identity changes: a fresh boot id means the binary restarted with new code,
//! and a fresh session mtime means the transcript grew, so a live session and
//! a live edit loop both refresh on their own.

use std::{
    path::Path,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};
use maud::html;
use tiny_http::{Header, Response, Server};

const LIVE_RELOAD: &str = r#"<script>
(function () {
  let last = null;
  async function poll() {
    try {
      const res = await fetch("/livereload", { cache: "no-store" });
      const now = await res.text();
      if (last !== null && now !== last) {
        location.reload();
        return;
      }
      last = now;
    } catch (_) {}
    setTimeout(poll, 500);
  }
  poll();
})();
</script>"#;

/// Serves the folio at `http://127.0.0.1:<port>`, re-rendering `render` on
/// every full page load and reloading the browser when `session` changes or
/// the server restarts. Runs until interrupted.
pub fn run(port: u16, session: &Path, render: impl Fn() -> Result<String>) -> Result<()> {
    let boot = now_nanos();
    let address = format!("127.0.0.1:{port}");
    let server = bind(&address)?;
    println!("serving http://{address}  (Ctrl-C to stop)");

    for request in server.incoming_requests() {
        if request.url() == "/livereload" {
            let token = format!("{boot}:{}", mtime_nanos(session));
            let _ = request.respond(Response::from_string(token));
            continue;
        }

        let body = match render() {
            Ok(page) => inject(&page),
            Err(error) => error_page(&error),
        };
        let _ = request.respond(Response::from_string(body).with_header(html_content_type()));
    }
    Ok(())
}

/// Binds the port, retrying briefly so a restart can reclaim it from the
/// previous run's lingering connections before giving up.
fn bind(address: &str) -> Result<Server> {
    let mut last = None;
    for _ in 0..40 {
        match Server::http(address) {
            Ok(server) => return Ok(server),
            Err(error) => {
                last = Some(error.to_string());
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(anyhow!(
        "could not bind {address}: {}",
        last.unwrap_or_default()
    ))
}

fn inject(page: &str) -> String {
    match page.rfind("</body>") {
        Some(index) => format!("{}{}{}", &page[..index], LIVE_RELOAD, &page[index..]),
        None => format!("{page}{LIVE_RELOAD}"),
    }
}

/// A minimal error page that keeps the reload loop alive, so a transient parse
/// failure (a session line half-written by a live session) recovers on the
/// next change instead of killing the server.
fn error_page(error: &anyhow::Error) -> String {
    let body = html! { pre { (error.to_string()) } };
    format!(
        "<!doctype html><html><body>{}{}</body></html>",
        body.into_string(),
        LIVE_RELOAD
    )
}

fn mtime_nanos(session: &Path) -> u128 {
    std::fs::metadata(session)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

fn html_content_type() -> Header {
    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
        .expect("static content-type header is valid")
}
