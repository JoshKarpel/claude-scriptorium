# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2]

### Added

- `[package.metadata.binstall]` so `cargo binstall claude-scriptorium` installs
  a prebuilt binary from the GitHub release instead of compiling from source.
- Turn numbers are now deep links: each `#N` points at its own panel
  (`#turn-N`), so a number is a shareable permalink and opening a folio at
  `#turn-N` scrolls to that panel, which takes a faint gilt highlight.
- A jump-to-end button and a follow (`tail -f`) toggle in the navigation dock.
  Following re-pins the newest message's start on every reload (so a live
  session tracked with `serve` stays at the latest), until the reader scrolls
  away. Jumping to the end also starts following.

### Changed

- The versal drop cap that opens each message is now gilded: a gold-leaf
  silhouette, lit diagonally, hugs the letter, which keeps its speaker colour.
  The fold-marker and divider fleurons take the same gold-leaf sheen.
- Marginal drolleries are mirrored at random, so neither illuminated border
  faces a single consistent direction.
- Copy buttons now stay visible (muted until hovered) for discovery instead of
  appearing only on hover. A turn's copy button rides its leading block's top
  edge, centred on the border, flush-right under the turn number; code and output
  blocks keep theirs in the top-right corner. Panel spacing is arranged so a
  button never covers text.
- The deep-link `:target` wash now extends as far right of the turn number as
  the text is inset from the border bar, so it reads symmetric.
- The folio-details plaque reveals on hover or keyboard focus, not only on
  click.

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
