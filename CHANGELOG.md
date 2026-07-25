# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2]

### Added

- `[package.metadata.binstall]` so `cargo binstall claude-scriptorium` installs
  a prebuilt binary from the GitHub release instead of compiling from source.

## [0.1.1]

### Added

- `gists` subcommand lists the gists this tool has published.
- `delete` subcommand removes a published gist by id or URL, or every published
  gist with `--all` (listing and confirming them first). It refuses any gist
  this tool did not publish, so it can never remove an unrelated gist.

### Changed

- `publish` is now idempotent per session. Each gist is stamped with a marker
  (the package name) and the session id, with its file named
  `<session-id>.html`, so re-publishing a session edits the existing gist in
  place and keeps its URL stable instead of piling up duplicates. A republish
  that would flip a gist between secret and public fails rather than silently
  ignoring the request.
- `publish` prints the preview link by default, alongside a note that only a
  reader's browser (never the viewer's host) fetches the transcript.

### Removed

- The `publish --preview` flag and its confirmation prompt; the preview link now
  prints by default.

### Fixed

- The gist viewer (`docs/index.html`) now calls `document.close()` after
  writing a folio. Writing from the async fetch callback left the parser open,
  so the folio stayed in `readyState: "loading"` and its `DOMContentLoaded`
  never fired, leaving search, copy buttons, the theme toggle, and the
  navigation dock dead when a session was viewed through GitHub Pages. Closing
  the document ends the parse and fires the event, so the app script wires up.

## [0.1.0]

Initial release.
