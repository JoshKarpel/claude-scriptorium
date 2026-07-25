# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3]

### Added

- Every built-in tool is now set in a shape that suits it, rather than falling
  back to raw JSON beyond `Bash`, `Write`, `Edit`, and `TodoWrite`. A plan, a
  subagent prompt, a skill's arguments, and a message to another agent are the
  markdown documents they were composed as; a workflow's script is highlighted
  JavaScript; a question shows every option it offered; a report shows each
  finding against the file and line it is about. A tool with no view of its own
  (an MCP server's, say) still shows the input it was sent.
- A question shows the preview an option carried, which is the mockup the reader
  actually compared, and its answer is recovered from the sentence the harness
  buries it in, so what was chosen reads as a line under what was asked rather
  than as a paragraph naming every question back. An answer typed instead of
  chosen is marked as one. A question that timed out is not an answer, and
  stands as the note it is.
- A result is now set by the tool that produced it: a read comes back as the
  file's own language, a search as the links it found, a background task as its
  status and its output, and an answer that is JSON comes back pretty-printed. A
  failure sheds the tag the harness wraps it in.
- Terminal colour is kept. A tool's output carries the ANSI escapes it was
  written with, so a test run that marked its failures in red now reads that way
  in the folio instead of showing the escapes. The sixteen colours a terminal
  names are ground into the folio's own pigments; a colour a tool states
  outright (256-colour or 24-bit) is carried as the value it asked for, since no
  palette token can stand for it. Escapes that drive the terminal rather than
  colour it (cursor moves, erases) leave nothing behind.
- `tests/fixtures/playground.jsonl` renders one panel per built-in tool, for
  looking at every view at once.

### Removed

- A result that says only that its call was carried out ("the file has been
  updated successfully", "launching skill", "entered plan mode", "async agent
  launched successfully") no longer appears: the call above it already shows the
  file it wrote or the change it made. Anything the result adds keeps it, so an
  edit that also warns the file changed on disk still reaches a reader, and a
  failure is always shown.
- The effort level a turn ran at, in parentheses after the model name, where
  the transcript records it.
- Token usage, where the transcript records it: each turn's meta line carries
  its input and output, faint until the panel is hovered, and the folio's plaque
  carries the session's flux. Hovering either gives the exact counts, each
  naming the scope it covers. A turn counts only the input it added, since every
  request re-sends the whole conversation and a turn's own output is what that
  stands against. A session's output totals, while its input is the largest
  single turn's: how big the conversation ever got, rather than a sum that would
  count the same text once per turn that saw it. One API response is written to
  the transcript a block at a time, each line repeating the response's usage, so
  a response is counted once.

### Changed

- The plaque's colophon links the tool's name to its repository, so a folio
  says where it came from.
- A marginalia body's copy button rides the rule between the summary and the
  body, centred on it, the way a turn's button rides its leading block's edge.
- A tool call's body fills its fold the way a result's does, instead of sitting
  in a second box inside it. What labelled the body from inside now labels the
  fold from its summary line: a `Bash` call is headed by its own description
  (falling back to the command), and an `Edit` that replaces every occurrence
  says so beside the file it edits.
- A call its summary line states in full is set as one flat line with no fold to
  open, since there is no subject left for a body to hold. A read is the common
  case: it names the file and the lines it took, and its contents arrive in the
  result below. Such a line wraps rather than ellipsising, because it is the
  only place the subject appears: a folded call can be cut short at the column's
  edge since opening it shows the subject in full, and a flat one can't.

### Fixed

- A code body no longer ends on an empty line. A file's own trailing newline is
  a fact about the file rather than a line of it, so setting it as one left a
  blank line against the bottom edge of the fold, reading as content that isn't
  there.
- Opening a folio at a `#turn-N` deep link now lands on that panel even when
  follow (`tail -f`) mode was left on in a previous session. An anchored load
  counts as the reader taking control, the same way scrolling does, so follow
  switches off instead of snapping past the linked panel to the end.
- A code or output block's copy button stays in the block's corner when the
  block is scrolled sideways, instead of travelling with the text and coming to
  rest over it.

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
