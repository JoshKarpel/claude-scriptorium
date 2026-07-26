# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.5]

### Added

- A minimap at the foot of the reading rail: the whole session seen edge-on, a
  band per message in that message's own pigment and sized to the share of the
  folio it takes, with your place on the leaf drawn over it. Drag along it to
  travel, and turn the wheel over it to zoom the map alone, so a long session's
  messages come apart far enough to pick one out without moving from where you
  are reading. It answers to the key like everything else in the rail, fading
  the kinds you have set aside. It is drawn as the book itself rather than as
  another card: the volume shut and lying spine to the left, seen from just
  above its front board, so the board recedes to a shallow trapezoid at the top,
  the back board's edge shows at the foot, and the painted edges of a great many
  leaves fill the space between.
- A folio now remembers more of how you left it, each under the session it
  belongs to: which kinds the key leaves in play, and how the minimap was
  framed, alongside the folds and the follow mode it already kept.
- The line a tool search's answer opens on names the tools it found. It came
  back blank, since a search answers with references rather than with text.
- The light a folio is read by is drawn in the corner, and is also the control
  that chooses it: press the sun to read by day, the candle to read after dark.
  Whichever light is in force is the one burning, so by day the sun turns its
  rays and the candle stands smoking, and after dark the moon hangs among its
  stars and the flame gutters; each casts a faint warm glow across the leaf, so
  the corner reads as where the page's light comes from. A small reset appears
  once a light has been chosen, and hands the choice back to the reader's own
  system. Both lights are drawn into every folio and the scheme lights one, so a
  folio still reads either way, and neither moves for a reader who asks for less
  motion.
- What the harness writes into a session is now on the page rather than hidden
  or dropped: a hook's output, a `CLAUDE.md` or rule file pulled into context,
  the instructions a skill or custom slash command carries, the slash command
  itself, the plan-mode boundaries, and a file edited outside the session or
  attached to it. Each gets its own quiet panel, a **gloss**, labelled by what
  wrote it there, with its content folded away behind a summary line so it
  annotates the conversation rather than crowding it. A reader can now see why
  the assistant did what it did, and not just what it did. Searching the notes
  is its own scope, and they are never navigation targets.
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

- Every kind of panel now carries its own pigment, so a reader scrolling can
  tell them apart without reading the label: a tool call takes the one cool hue
  nothing else holds, reasoning takes the assistant's own drawn back toward the
  ink, and the harness's notes take malachite for a hook, ochre for a skill or
  command, and rubricated vermilion for a plan boundary. A rule pulled into
  context and a passing note stay in faint ink, since colouring every kind would
  leave nothing quiet. Tool and thinking labels were previously muted to a flat
  grey, which left them the plainest things on the page.
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
- A slash command and what it printed are one panel. The harness records the
  output as a line of its own, which stood as a second panel whose whole summary
  was the word "output"; the two are now joined on the line the harness itself
  names as the parent. A command that only works the harness takes its output
  with it when it goes.
- The folio's colours run on one axis: **warm is what the model produced, cool
  is what reached it from outside.** So a tool call takes the ochre its own name
  is set in rather than a cool hue, a skill takes the teal it vacates, and a
  reader scrolling learns which side of the exchange they are passing before
  reading a label. A plan boundary stays rubricated and the ambient kinds stay
  in faint ink, deliberately off the axis.
- The dock steps along that same axis: the cool arrows seek what reached the
  model (your words, commands, skills, and hooks) and the warm ones what it
  produced (replies, reasoning, tool calls), where before they sought one
  speaker and skipped everything else.
- Every landing names its turn in the URL, so a reload returns you to where you
  were reading and the position is a link you can share. Navigation also lands
  at once instead of gliding, the leaps to either end included: over a folio
  megabytes tall a smooth scroll is an animation to sit through, and one that a
  live session's re-render can interrupt and lose.
- The reading controls stand together in one column down the right, led by the
  key, with the search and the navigation dock under it. They were scattered to
  opposite corners, which hid that they are one mechanism.
- The folio has a **key**: a chip per kind of panel, in its own card above the
  search rather than inside it, set as a column per side of the exchange and
  carrying each kind's own pigment, so it says what every edge in the margin
  means as well as which kinds are wanted. The search and the navigation arrows
  both answer to it, so narrowing it to skills searches skills and steps through
  skills alike: one place to say what you are reading through, rather than one
  per control that reads.
- A gloss's edge is solid rather than dotted. The dots said "nobody's speech"
  back when the notes shared a few pigments between them; now each kind has its
  own, they were a second mark for something already said.
- A hook keeps the line breaks it printed. Its injected context was set as
  ordinary markdown, which folds a single newline into a space, so a hook
  reporting on a working tree came out as one unreadable run of filenames. It
  sits between the two readings and is now set as such: read as markdown, so
  headings and lists survive, but keeping its own breaks.
- A rule takes a cool pigment of its own rather than the neutral ink, since it
  is a file you wrote: the skill's teal, drawn back toward the ink because
  rules arrive a dozen at a time where a skill arrives singly.
- Output that redrew itself reads as the terminal left it. A spinner or a
  progress bar emits a frame per carriage return, overwriting its line each
  time; the folio set every frame instead, running dozens of them together into
  one line with no breaks, which is the shape a build log most often takes.
- A built-in skill reads as a skill. One with a directory on disk names it, and
  that name is how a skill was recognised; a built-in (`/review`, `/init`,
  `/security-review`) has no directory, so its instructions were set as an
  anonymous passing note. The slash command in front of them now names them, so
  a skill looks the same whether a command loaded it or the assistant reached
  for it unbidden.
- A slash command takes the reader's own pigment rather than the skill's. The
  harness wrote it into the session, but the user typed it: it is the only note
  in a folio that anyone actually spoke, and sharing a skill's ochre said
  otherwise just where a command stands in front of the skill it loaded.
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
- One firing of a hook is one panel, however many lines it wrote. A hook that
  answers in the control protocol records what it decided and what it injected
  separately, which stood as two panels each saying half of it; they are now
  gathered the way a tool result is gathered into the call it answers, with the
  decision on the summary line and the injected context in the fold.
- A slash command that works the harness rather than the conversation (`/copy`,
  `/config`, `/resume`, and the like) is no longer set. The transcript records
  every slash command alike, whether it injected a whole skill or only redrew
  the screen, and being told that the last reply went to the clipboard tells a
  reader nothing. The ones that change the conversation stay, `/compact` and
  `/model` among them.
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
- The labels in the key sat visibly high in their pills.
- The cut faces carry the angle brackets and arrows that sessions turned out to
  write (`⟨these⟩`, `⬆`), so a folio using one no longer falls back to embedding
  the whole faces and quadrupling in size. The two blocks cost well under a
  kilobyte between them.
- A slash command reads as the command it is. The harness records one as a turn
  wrapping its name, arguments, and output in XML-ish tags, and a folio set those
  tags as literal text in the middle of the conversation; the caveat standing in
  front of it was set as a paragraph of the user telling itself not to answer.
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
