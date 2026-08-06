// The folio's functional core: the arithmetic its controls are built on, kept
// apart from the shell that reads the document and moves it.
//
// Nothing here touches the DOM, the clock, or storage: every function takes what
// it needs and answers with a value, so each one can be exercised without a
// browser (`tests/js`, run by `just test-js`) while the behaviour it drives is
// exercised in one (`tests/browser`, run by `just test-browser`). It is inlined
// into the same script tag as `illumination.shell.js`, ahead of it, so the shell
// simply closes over `core`.
const core = {
  // --- What the reader is remembered by --------------------------------

  // The theme is the reader's and holds across every folio; anything else is a
  // fact about one folio and is stored under the session the markup names, since
  // every folio opened from disk shares the `file://` origin and every folio
  // served through one viewer shares that host.
  perFolio(store, folio) {
    return store + ":" + (folio || "?");
  },

  // Storage is arbitrary text: a value written by an older folio, or by a hand
  // in the console, must not leave the page in a scheme it has no rules for.
  theme(stored) {
    return ["system", "light", "dark"].includes(stored) ? stored : "system";
  },

  // A fold's own key is a turn number and a position within that turn, which is
  // stable as a live session grows (the raw stream only ever appends) and names
  // a different marginalia in every other session.
  foldKey(turn, index) {
    return `${turn === undefined ? "?" : turn}:${index}`;
  },

  // How the map was last framed, parsed out of whatever the store holds: the
  // zoom, and where the lens was pointed as a *fraction* of the document rather
  // than a position in it. A folio is a different length on every load (a
  // session that grew, a window of another width), and what a reader framed is a
  // place in the folio, not a number of pixels. Anything unparseable is the
  // whole folio, which is what a map shows until it is zoomed.
  framing(stored) {
    let held;
    try {
      held = JSON.parse(stored);
    } catch {
      return { zoom: 1, at: 0 };
    }
    const zoom = Number(held?.zoom);
    const at = Number(held?.at);
    return {
      zoom: Number.isFinite(zoom) ? Math.max(zoom, 1) : 1,
      at: Number.isFinite(at) ? Math.min(Math.max(at, 0), 1) : 0,
    };
  },

  // --- What arrives while the folio is open -----------------------------

  // Where a panel belongs among the ones already on the leaf, as an index into
  // them: which of them it goes in front of, or their count to say it goes last.
  //
  // A panel is placed by its turn number rather than appended, because a session
  // does not only grow at the end: a tool result joins the panel holding its
  // call, and a gloss the harness wrote is gathered into the panel it belongs to,
  // so a folio can be told about a panel that belongs in the middle of what the
  // reader already has. Turn numbers count the raw records and never change, so
  // they order the leaf on their own.
  seatFor(turns, turn) {
    const at = turns.findIndex((held) => held > turn);
    return at === -1 ? turns.length : at;
  },

  // Whether the server saying hello is a different run of it from the one this
  // page was set by, which is the only thing a page cannot patch its way through:
  // the stylesheet, the script, and the renderer are all part of the page rather
  // than of the session, so a new binary means a reload. The first hello is what
  // the page was set by, since the page and that greeting came from one run.
  restarted(seen, boot) {
    return Boolean(seen) && Boolean(boot) && seen !== boot;
  },

  // --- The hash, which two hands write ---------------------------------

  // The hash is arbitrary text off the end of a shared URL, so it need not be
  // validly escaped: a stray "%" throws rather than decoding.
  anchorId(hash) {
    const raw = hash.startsWith("#") ? hash.slice(1) : hash;
    try {
      return decodeURIComponent(raw);
    } catch {
      return raw;
    }
  },

  // Whether a hash is the reader naming where they want to be, rather than the
  // folio naming where it has put them. Following writes the newest panel's
  // permalink and remembers it, so the pin is what tells the two apart on the
  // next load. Reading every hash as the reader's meant following survived
  // exactly one reload of a growing session, and none at all once a step of the
  // dock had left a permalink in the URL.
  readersHash(hash, pinned) {
    const id = core.anchorId(hash);
    return Boolean(id) && id !== pinned;
  },

  // --- Stepping ---------------------------------------------------------

  // Which panel the reader is on, and which one a step lands on. The panel at
  // the top of the viewport is the last one whose top has scrolled to or above
  // the threshold, which clears a turn's own scroll-margin so the one just
  // navigated to counts as current rather than as the next one back.
  currentIndex(tops, threshold) {
    let current = -1;
    tops.forEach((top, index) => {
      if (top <= threshold) current = index;
    });
    return current;
  },

  // Clamped rather than wrapped: an arrow at the end of a folio is spent, and
  // wrapping would take a reader who pressed it once too often back to the
  // opposite end of a session they had just read through.
  stepIndex(tops, direction, threshold) {
    const current = core.currentIndex(tops, threshold);
    return Math.min(Math.max(current + direction, 0), tops.length - 1);
  },

  // --- The minimap's geometry -------------------------------------------

  // The lens the map is drawn through: which stretch of the leaf the track
  // shows, and at what scale. A zoom of 1 is the whole folio, which is what a
  // map is for, so it is also the floor; the origin is clamped so the lens can
  // never be pointed off either end of the document.
  lens({ leaf, track, zoom = 1, origin = 0 }) {
    const held = Math.max(zoom, 1);
    const whole = leaf || 1;
    const span = whole / held;
    return {
      leaf: whole,
      track,
      zoom: held,
      span,
      origin: Math.min(Math.max(origin, 0), Math.max(whole - span, 0)),
      scale: track / span,
    };
  },

  // Zooming holds still whatever the pointer is over, the way a map does, so a
  // reader points at the stretch they mean to look into rather than zooming the
  // middle and then hunting for it.
  zoomedAbout(lens, y, factor, most) {
    const zoom = Math.min(Math.max(lens.zoom * factor, 1), most);
    const under = lens.origin + y / lens.scale;
    return core.lens({
      leaf: lens.leaf,
      track: lens.track,
      zoom,
      origin: under - (y / lens.track) * (lens.leaf / zoom),
    });
  },

  // Keeps the reader's own view within the stretch the lens shows, by nudging
  // the lens rather than by moving the reader: zoomed in, a folio scrolled far
  // enough would otherwise leave the map showing somewhere the reader is not.
  followed(lens, scrollY, viewport) {
    const end = lens.origin + lens.span;
    if (scrollY >= lens.origin && scrollY + viewport <= end) return lens;
    return core.lens({
      ...lens,
      origin: scrollY + viewport / 2 - lens.span / 2,
    });
  },

  // A band states the share of the lens its panel takes, so the map is the
  // document to scale. The floor is what keeps a one-line note somewhere to aim
  // at in a folio whose tool output runs to thousands of lines.
  bandBox(top, height, lens, floor) {
    return {
      top: (top - lens.origin) * lens.scale,
      height: Math.max(height * lens.scale, floor),
    };
  },

  // The same arithmetic for what the reader has in front of them. Its own floor
  // is larger: a folio of any length puts the whole viewport into a pixel or
  // two, and a mark that thin is no mark at all.
  viewBox(scrollY, viewport, lens, floor) {
    return {
      top: (scrollY - lens.origin) * lens.scale,
      height: Math.max(viewport * lens.scale, floor),
    };
  },

  // Where a scrub lands: the nearest band the key leaves in play, so a drag
  // across a stretch the reader has filtered out still puts them somewhere they
  // asked to see rather than nowhere. Ties go to the earlier band, which is the
  // one the reader has already passed.
  nearestIndex(y, bands) {
    let found = -1;
    let gap = Infinity;
    bands.forEach((band, index) => {
      if (!band.inPlay) return;
      const distance = Math.abs(band.top + band.height / 2 - y);
      if (distance >= gap) return;
      gap = distance;
      found = index;
    });
    return found;
  },

  // --- Searching --------------------------------------------------------

  // Where a needle falls in a haystack, case-insensitively. The spans are what
  // the caller cuts the text node at; finding them is the whole of the search
  // that does not need a document.
  spans(text, needle) {
    const found = [];
    if (!needle) return found;
    const hay = text.toLowerCase();
    const query = needle.toLowerCase();
    let from = hay.indexOf(query);
    while (from !== -1) {
      found.push({ from, to: from + needle.length });
      from = hay.indexOf(query, from + needle.length);
    }
    return found;
  },
};
