# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`codex`**, a server for every session the machine has recorded: a listing of
  each project and the sessions in it, most recently worked in first, with the
  session being written right now marked as such, and any of them one click away
  as a folio. `--host` binds it somewhere other than localhost, for a machine
  that has something in front of it to say who may read (a reverse proxy that
  authenticates); `--root` lists a projects root other than Claude Code's own.

### Changed

- **A served folio gains a panel in place rather than reloading.** A panel's id
  is its turn number, which counts a session file's raw records and so never
  changes, and the server compares the panels it has just set with the ones it
  last sent and pushes only those that differ. So a reader keeps their scroll
  position, their open folds, their search and its count, and their place in the
  conversation, where the reload this replaced rebuilt megabytes of markup to add
  one panel and threw all of that away. A listing keeps itself current the same
  way. Both listen on one server-sent event stream, named in the page itself.
- **A served folio links its stylesheet, script, and fonts instead of inlining
  them**, under URLs that name their contents so they can be cached forever. The
  faces are most of a folio, so a session opened out of a codex is now a fraction
  of what it was: browsing no longer re-downloads a megabyte of fonts per session.
  A folio *written* to a file or published as a gist is unchanged, still carrying
  every byte it needs.
- `serve` is the same server as `codex` with one session in scope, so the render
  loop gains the push and neither can drift from the other. It keeps `--port` and
  gains `--host`; a change to the renderer, the stylesheet, or the app script
  still reaches the page, by the page noticing the server restarted and reloading
  itself.
- Both servers default to port 8000, which is what a proxy in front of a
  development machine is most likely to be expecting already: exe.dev's HTTPS
  proxy, for one, forwards it without being told to.
- A session caught mid-write no longer shows the reader a parse failure: the read
  is retried, and a followed folio simply is not told anything until a whole
  setting succeeds.

## [0.1.6]

### Fixed

- `fetch` and re-`publish`ing a session both work against a GitHub Enterprise
  instance. Both failed there with `406 Not Acceptable`, which reads as a
  quarrel about content types and is really an authentication failure: a folio
  is over the gists API's ~1 MB limit, so the API answers a read with a
  `raw_url` instead of the content, and on an enterprise instance that URL is
  served by the web app, which wants a session cookie rather than an API token.
  Nothing now reads a folio through that URL. A republish writes through the API
  in one request, without first reading back the file it is replacing, and
  `fetch` clones the gist as the git repository it is, which has no size limit
  and authenticates as any other clone does. Both paths change on `github.com`
  too, where the raw URL happened to work; `fetch` now needs `git` on the PATH.

### Changed

- `scaffold-viewer` reads the host from `gh` when `--host` is not given, so a
  viewer scaffolded on a machine that publishes to an enterprise instance points
  at that instance rather than at github.com. It therefore needs `gh` to be
  authenticated, unless `--host` says which instance to scaffold for.
- A scaffolded viewer's README names `CLAUDE_SCRIPTORIUM_VIEWER_BASE`, so the
  viewer can be pointed at once rather than per publish, and, for an enterprise
  host, says what will stop the viewer working there (an instance in private
  mode, and a folio over the API's size limit) and that `fetch --open` reads a
  folio regardless.

## [0.1.5]

### Added

- A folio opens with a **caveat** saying what a session file cannot show: the
  system prompt, the tool descriptions, and the `CLAUDE.md` and rule files
  loaded when a session starts are sent with every request but never recorded,
  so the transcript is the conversation and not the whole of what shaped it. It
  is the folio's own voice rather than anyone's turn, so it is drawn as a
  rule-flanked note rather than a panel, and nothing that reads panels counts it
  as one: the dock won't step to it, the minimap draws no band for it, the key
  can't set it aside, and the search never returns it as a hit.
- A minimap at the foot of the reading rail: the whole session seen edge-on, a
  band per message in that message's own pigment and sized to the share of the
  folio it takes, with your place on the leaf drawn over it. Drag along it to
  travel, and turn the wheel over it to zoom the map alone, so a long session's
  messages come apart far enough to pick one out without moving from where you
  are reading. It answers to the key like everything else in the rail, fading
  the kinds you have set aside. It is drawn as the book itself rather than as
  another card: the volume shut and lying spine to the left, seen from above its
  front board and off to the spine side, so the board and the spine each open
  into a face of their own, the back board's edge shows at the foot, and the
  painted edges of a great many leaves fill the space between.
- A folio now remembers more of how you left it, each under the session it
  belongs to: which kinds the key leaves in play, and how the minimap was
  framed, alongside the folds and the follow mode it already kept.
- The light a folio is read by is drawn in the corner, and is also the control
  that chooses it: press the sun to read by day, the candle to read after dark.
  Whichever light is in force is the one burning, so by day the sun turns its
  rays and the candle stands smoking, and after dark the moon hangs among its
  stars and the flame gutters; each casts a faint warm glow across the leaf, so
  the corner reads as where the page's light comes from. A small reset appears
  once a light has been chosen, and hands the choice back to the reader's own
  system. Both lights are drawn into every folio and the scheme lights one, so a
  folio still reads either way, and neither moves for a reader who asks for less
  motion. This replaces the light/dark/system toggle that stood under them:
  there is nothing left to label, because the lights are the control.
- What the harness writes into a session is now on the page rather than hidden
  or dropped: a hook's output, a `CLAUDE.md` or rule file pulled into context,
  the instructions a skill or custom slash command carries, the slash command
  itself, the plan-mode boundaries, and a file edited outside the session or
  attached to it. Each gets its own quiet panel, a **gloss**, labelled by what
  wrote it there, with its content folded away behind a summary line so it
  annotates the conversation rather than crowding it. A reader can now see why
  the assistant did what it did, and not just what it did.

  One event is one panel however many lines the harness spent on it, so a hook
  states what it decided on its summary line and what it injected in its fold,
  and a slash command carries what it printed. A skill reads the same whether a
  command loaded it or the assistant reached for it unbidden, including the
  built-ins (`/review`, `/init`, `/security-review`) that have no directory to be
  known by. A command that works the harness rather than the conversation
  (`/copy`, `/config`, `/resume`, and the like) is left out, along with what it
  printed: the transcript records every slash command alike, and being told that
  the last reply went to the clipboard tells a reader nothing. The ones that
  change the conversation stay, `/compact` and `/model` among them.
- A copy button scratches like a quill taking the passage down: a short word of
  five to eight strokes, the pen lifted between them, each stroke its own
  length, weight, and tone, so no two words are written the same way. The sound
  is synthesized in a few lines rather than embedded as a recording, so a folio
  stays one file that carries every byte it needs. Nothing waits on it: the pen
  is readied when a copy button is first hovered, and the sound is made after
  the copy is already underway.
- A folio is scrolled by a scroll. The scrollbar thumb is a sheet of parchment
  wound onto two turned rollers, scratched over with lines of writing, and it
  lengthens and shortens with the document the way a real one does; it lies on
  its side for a code block that runs off the edge. The bar never narrows below
  the platform default and is never hidden, since it is both the position
  indicator and the drag target, and a reader in a high-contrast mode gets
  their own system's scrollbar instead of this one.

### Changed

- Every kind of panel carries its own pigment, and they all run on one axis:
  **warm is what the model produced, cool is what reached it from outside.** A
  reader scrolling therefore learns which side of the exchange they are passing
  before reading a label. The assistant speaks in its own orange, reasons in that
  orange drawn back toward the ink, and reaches for a tool in ochre; you speak in
  lapis, type a command in that lapis drawn back toward the ink, and your skills,
  rules, and hooks arrive in teal and malachite. A plan boundary is rubricated
  instead, marking a division in the text rather than anything said in it, and
  the catch-all note stays in faint ink so the rest can be loud. Tool and
  thinking labels were previously muted to a flat grey, which left them the
  plainest things on the page.
- The dock steps along that same axis: the cool arrows seek what reached the
  model (your words, commands, skills, and hooks) and the warm ones what it
  produced (replies, reasoning, tool calls), where before they sought one
  speaker and skipped everything else.
- The folio has a **key**: a chip per kind of panel, in its own card above the
  search rather than inside it, set as a column per side of the exchange and
  carrying each kind's own pigment, so it says what every edge in the margin
  means as well as which kinds are wanted. The search, the navigation arrows, and
  the minimap all answer to it, so narrowing it to skills searches skills, steps
  through skills, and fades the rest of the map alike: one place to say what you
  are reading through, rather than one per control that reads.
- The reading controls stand together in one column down the right, led by that
  key, with the search, the navigation dock, and the minimap under it. They were
  scattered to opposite corners, which hid that they are one mechanism.
- A tool result is shown against the call it answers. Calls issued together are
  recorded one line each, so they became several panels and every result piled
  onto the last of them while its siblings showed none: a batch of five searches
  put all five results under the fifth. Each result now joins its own call's
  panel.
- A result's summary line previews what came back rather than saying "result".
  Since each result now sits with its own call, the box above it already names
  the tool and its subject, so the line shows the first thing the tool actually
  said instead: what a command printed, a file's opening line, the option that
  was chosen. Only a failure is still named, being the exception worth marking.
- Every landing names its turn in the URL, so a reload returns you to where you
  were reading and the position is a link you can share. Navigation also lands
  at once instead of gliding, the leaps to either end included: over a folio
  megabytes tall a smooth scroll is an animation to sit through, and one that a
  live session's re-render can interrupt and lose.
- Output that redrew itself reads as the terminal left it. A spinner or a
  progress bar emits a frame per carriage return, overwriting its line each
  time; the folio set every frame instead, running dozens of them together into
  one line with no breaks, which is the shape a build log most often takes.
- A fold whose body is prose carries a copy button, so a skill's instructions, a
  rule pulled into context, a plan, or a subagent's prompt can be lifted out
  whole. Only code and output blocks had one.
- Prose in a fold is set at the size the conversation is. It took the marginalia's
  smaller measure, which suits a summary line and a list of facts but not a
  skill's whole instructions, which are read rather than scanned.
- Inline code breaks rather than running out of whatever holds it. A single
  unbreakable token (a flag's comma-separated values, a deep path) had nowhere
  to go, since inline code has no scroller of its own the way a block does, and
  it showed worst inside a fold, where the body is the box.
- The folio is set a little larger, and its head and foot sit closer to the
  first and last panel than its illuminated margins do to the text.
- The dock's follow control is set only into a folio that `serve` is serving.
  Only that folio is re-read and re-rendered as the session is written; a folio
  written to a file or published as a gist is a snapshot, so following it
  promised an update that could never arrive. Jumping to the end still works
  everywhere, and is now just a jump there rather than switching following on.

### Fixed

- Following the end of a live session survived at most one reload, and none at
  all once a step of the dock had left a permalink in the URL: the folio read
  the hash it had written itself as a reader arriving at a turn, and the browser
  restored the scroll position from before the reload over the top of it.
  Following now keeps the newest message named in the URL as the session grows,
  so a reload resumes at the end and a link copied out of a followed folio names
  what was on the screen.
- The gilt wash marking where you landed appeared only when you arrived by a
  link, and then stayed on that message through every step afterward. Every way
  of arriving at a message now marks it, and only it.
- The cut faces carry the angle brackets and arrows that sessions turned out to
  write (`⟨these⟩`, `⬆`), so a folio using one no longer falls back to embedding
  the whole faces and quadrupling in size. The two blocks cost well under a
  kilobyte between them.
- A slash command reads as the command it is. The harness records one as a turn
  wrapping its name, arguments, and output in XML-ish tags, and a folio set those
  tags as literal text in the middle of the conversation; the caveat standing in
  front of it was set as a paragraph of the user telling itself not to answer.
- A folio rendered on Windows is the same file as one rendered anywhere else.
  The stylesheet and the app script are inlined verbatim from the source tree, so
  a checkout that rewrote their line endings carried those endings into every
  folio it wrote.
- A marginalia left open in one folio no longer opens panels in another, and
  following the end of a live session no longer snaps an unrelated folio to its
  end. Both were kept in one store shared by every folio on an origin, and a
  fold's key is a turn number and a position within that turn, which names a
  different marginalia in every session: opening a second folio from disk, or
  through the same viewer, imposed the first one's state on it. Each folio now
  remembers its own, under the session its markup names. The theme is still the
  reader's and still holds across every folio.

## [0.1.4]

### Added

- A folio's plaque states what the render cost: how long the scribe took, and
  how large the folio came out. `render`, `serve`, and `publish` report the same
  two figures on stderr, leaving stdout the folio's path alone.
- `--whole-fonts` on `render`, `serve`, and `publish`, to embed the whole
  upstream faces whatever the session sets. Worth it for a folio that will later
  gain text the session did not have; a folio that already sets such a character
  switches on its own.

### Changed

- A folio carries fonts cut to what a transcript sets, which takes a typical one
  from ~3.1 MB to ~0.8 MB. The faces were ~98% of a short folio: Junicode ships
  3162 codepoints for medieval scholarship and varies on width and ENLA, none of
  which this project asks for. A folio whose text reaches a character the cut
  faces dropped carries the whole ones instead and says so on stderr, so cutting
  can never render a character worse than upstream would. Characters no face
  ever carried, an emoji or a CJK ideograph, still fall back to the reader's own
  fonts and do not grow the folio.
- `publish` says plainly that the gist page shows a folio's source rather than
  the folio, so the viewer link beneath it reads as the way to see it rather
  than an alternative to a page that already works.
- A render no longer base64s the embedded fonts. They are constants, so they are
  encoded into their `@font-face` block at compile time, and every render starts
  from the finished block.
- Panels are set in parallel. Almost all of a render is syntax highlighting, and
  almost all of that is a syntax's regexes compiling the first time its language
  is met, so compiling one language no longer holds up meeting the next: a
  session with code in a dozen languages renders about three times faster, one
  with a single language about as much again, and a session with no code blocks
  is unchanged. The folio itself is byte for byte what it was.

### Fixed

- Re-publishing a session no longer opens a text editor. The description was
  updated in a second `gh gist edit` call, and that command does not stop once
  it has set a description: it goes on to the file-edit loop, which with no
  source file opens `$EDITOR` against the piped stdin. The content and the
  description now go up in one call, which is also one request rather than two.

## [0.1.3]

### Added

- A jump-to-top button in the navigation dock, beside jump-to-end. Using it
  counts as the reader taking control, so it switches follow (`tail -f`) mode
  off rather than letting the next reload snap back to the end.
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
