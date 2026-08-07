// illumination: the folio's own behaviour, inlined into every written file and
// linked by every served one.
//
// This is the folio's one script, and it is trusted: transcript content is always
// escaped, never run, and so is the markup that arrives while a served folio is
// open. Keep it small and dependency-free so a written folio stays a single
// portable file.
//
// The imperative shell: everything that reads the document, listens for the
// reader, or writes to storage. The core it stands on is
// `illumination.core.js`, inlined ahead of this and answering as `core`.
// Anything that can be worked out from values belongs there, where it is tested
// without a browser; what is left here is the wiring.
(() => {
  "use strict";

  // --- Theme: light / dark / system -------------------------------------
  //
  // Every colour is a light-dark() pair resolved by `color-scheme`. Forcing a
  // side is just pinning color-scheme via [data-theme] on <html>; "system"
  // clears the attribute and falls back to the OS preference. The choice
  // persists in localStorage and is applied before first paint (this script
  // sits in <head>) so a forced theme never flashes the system one.

  const THEME_KEY = "scriptorium-theme";

  // The theme above is the reader's, and holds across everything they open.
  // What follows is about one folio and is stored under the session the markup
  // names: which marginalia stand open, and whether the reader is following the
  // end of the session. Following a session still being written says nothing
  // about a folio finished months ago, and every folio a reader opens from disk
  // shares the `file://` origin, as does every folio served through one viewer,
  // so an unscoped store is one folio's state imposed on all of them.
  const FOLDS = "scriptorium-folds";
  const TAIL = "scriptorium-tail";
  const MAP = "scriptorium-map";
  const KEY = "scriptorium-key";
  const perFolio = (store) => core.perFolio(store, document.body.dataset.folio);

  // Storage is a privilege a folio can be opened without (a `file://` page under
  // some settings has none at all), so every read of it answers with nothing
  // rather than throwing.
  const stored = (key) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  };

  // Keys whose default action scrolls the page, so pressing one counts as the
  // reader taking over from follow mode (unless focus is in a control).
  const SCROLL_KEYS = new Set([
    "ArrowUp",
    "ArrowDown",
    "PageUp",
    "PageDown",
    "Home",
    "End",
    " ",
  ]);

  const readTheme = () => {
    try {
      return core.theme(localStorage.getItem(THEME_KEY));
    } catch {
      return "system";
    }
  };

  const applyTheme = (theme) => {
    const root = document.documentElement;
    if (theme === "system") {
      delete root.dataset.theme;
    } else {
      root.dataset.theme = theme;
    }
  };

  applyTheme(readTheme());

  // The lights are the control: pressing one asks to read by it, and the reset
  // beside them hands the choice back to the system. Which light is *drawn* as
  // burning is the stylesheet's, off the scheme in force; all this does is say
  // which scheme that is, and record which button asked for it.
  const wireThemeToggle = () => {
    const toggle = document.querySelector(".luminaries");
    if (!toggle) return;
    const buttons = toggle.querySelectorAll("[data-theme-choice]");
    const sync = (theme) => {
      buttons.forEach((button) => {
        const active = button.dataset.themeChoice === theme;
        button.setAttribute("aria-pressed", String(active));
      });
    };
    sync(readTheme());
    buttons.forEach((button) => {
      button.addEventListener("click", () => {
        const theme = button.dataset.themeChoice;
        applyTheme(theme);
        try {
          localStorage.setItem(THEME_KEY, theme);
        } catch {}
        sync(theme);
      });
    });
  };

  // --- Search: highlight every match, step through with next / prev -------
  //
  // Non-destructive to the layout: nothing is hidden. Each query marks all
  // matches inside the conversation, keeps a running index, and scrolls the
  // current one into view, opening any collapsed marginalia that holds it so
  // the match is actually visible.

  const HIT = "search__hit";
  const CURRENT = "is-current";

  const clearHits = (container) => {
    container.querySelectorAll("mark." + HIT).forEach((mark) => {
      mark.replaceWith(document.createTextNode(mark.textContent));
    });
    container.normalize();
  };

  // Which kind of message a text node belongs to, judged by the block it sits
  // in rather than its panel's label: a tool call folded into an assistant
  // panel is still "tool", and reasoning is "thinking", so scoping is precise.
  // A gloss is judged by its panel's own kind and checked first, since its
  // content sits in a marginalia and would otherwise scope as a tool call, and
  // since each kind of note is its own chip.
  const scopeOf = (node) => {
    const el = node.parentElement;
    if (!el) return "assistant";
    const gloss = el.closest(".turn--gloss");
    if (gloss) return gloss.dataset.kind;
    if (el.closest(".marginalia")) return "tool";
    if (el.closest(".block--thinking")) return "thinking";
    const turn = el.closest(".turn");
    if (turn && turn.classList.contains("turn--user")) return "user";
    return "assistant";
  };

  const markHits = (container, query, scopes) => {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (!node.nodeValue.trim()) continue;
      // Copy-button labels are chrome, not transcript; don't match them.
      if (node.parentElement && node.parentElement.closest(".copy-button")) {
        continue;
      }
      // Restrict to the kinds of message the reader left enabled.
      if (scopes && !scopes.has(scopeOf(node))) {
        continue;
      }
      nodes.push(node);
    }
    const hits = [];
    for (const node of nodes) {
      const text = node.nodeValue;
      const spans = core.spans(text, query);
      if (!spans.length) continue;
      const frag = document.createDocumentFragment();
      let pos = 0;
      for (const { from, to } of spans) {
        if (from > pos) {
          frag.appendChild(document.createTextNode(text.slice(pos, from)));
        }
        const mark = document.createElement("mark");
        mark.className = HIT;
        mark.textContent = text.slice(from, to);
        frag.appendChild(mark);
        hits.push(mark);
        pos = to;
      }
      if (pos < text.length) {
        frag.appendChild(document.createTextNode(text.slice(pos)));
      }
      node.parentNode.replaceChild(frag, node);
    }
    return hits;
  };

  const revealHit = (hit) => {
    for (let node = hit.parentElement; node; node = node.parentElement) {
      if (node.tagName === "DETAILS") node.open = true;
    }
  };

  // The folio's key: which kinds of panel are in play. It is deliberately not
  // the search's own control. The search, the dock, and the minimap all read it,
  // so a reader says once what they are looking through rather than once per
  // control that looks. Read fresh each time rather than cached, so nothing has
  // to be told when a chip is pressed.
  const keyChips = () => document.querySelectorAll(".key__chip");

  const enabledKinds = () => {
    const kinds = new Set();
    keyChips().forEach((chip) => {
      if (chip.getAttribute("aria-pressed") === "true") {
        kinds.add(chip.dataset.scope);
      }
    });
    return kinds;
  };

  // The key owns the chips' state, and is the only thing that writes it: the
  // search, the dock, and the minimap each read it fresh and repaint on their
  // own listeners, which run after this one because it is wired first.
  //
  // What is stored is the kinds set *aside*, not the ones in play, so a kind
  // added to a later folio arrives in play rather than silently missing from a
  // reader's stored list.
  const wireKey = () => {
    const key = document.querySelector(".key");
    if (!key) return;
    const setAside = () =>
      new Set(
        Array.from(keyChips())
          .filter((chip) => chip.getAttribute("aria-pressed") !== "true")
          .map((chip) => chip.dataset.scope),
      );

    let aside;
    try {
      const held = JSON.parse(stored(perFolio(KEY)) || "[]");
      aside = new Set(Array.isArray(held) ? held : []);
    } catch {
      aside = new Set();
    }
    keyChips().forEach((chip) => {
      chip.setAttribute("aria-pressed", String(!aside.has(chip.dataset.scope)));
    });

    // Captured, not bubbled: the search and the minimap listen for the same
    // click, and a listener on the chip itself runs before one on the key that
    // waits for the click to reach it. They would then read the state this is
    // about to flip, and paint the press before it happened.
    key.addEventListener(
      "click",
      (event) => {
        const chip = event.target.closest(".key__chip");
        if (!chip) return;
        const active = chip.getAttribute("aria-pressed") === "true";
        chip.setAttribute("aria-pressed", String(!active));
        try {
          localStorage.setItem(perFolio(KEY), JSON.stringify([...setAside()]));
        } catch {}
      },
      true,
    );
  };

  const wireSearch = () => {
    const search = document.querySelector(".search");
    const container = document.querySelector("main.folio");
    if (!search || !container) return null;
    const input = search.querySelector(".search__input");
    const count = search.querySelector(".search__count");
    const prev = search.querySelector('[data-search-nav="prev"]');
    const next = search.querySelector('[data-search-nav="next"]');
    const chips = keyChips();

    let hits = [];
    let index = -1;

    // `land` is what separates a reader asking for a hit from the folio noticing
    // one. Going to a hit opens the folds around it and scrolls it into view;
    // merely re-counting must do neither, or a panel arriving would drag a
    // reader who is halfway down the hit list back to the first one.
    const paint = (land) => {
      hits.forEach((hit) => hit.classList.remove(CURRENT));
      const nav = hits.length > 0;
      prev.disabled = !nav;
      next.disabled = !nav;
      if (!input.value) {
        count.textContent = "";
        return;
      }
      if (!nav) {
        count.textContent = "no matches";
        return;
      }
      const hit = hits[index];
      hit.classList.add(CURRENT);
      if (land) {
        revealHit(hit);
        hit.scrollIntoView({ block: "center", behavior: "smooth" });
      }
      count.textContent = index + 1 + "/" + hits.length;
    };

    const mark = () => {
      clearHits(container);
      const query = input.value;
      hits = query ? markHits(container, query, enabledKinds()) : [];
    };

    const run = () => {
      mark();
      index = hits.length ? 0 : -1;
      paint(true);
    };

    const step = (delta) => {
      if (!hits.length) return;
      index = (index + delta + hits.length) % hits.length;
      paint(true);
    };

    input.addEventListener("input", run);
    input.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") return;
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
    });
    prev.addEventListener("click", () => step(-1));
    next.addEventListener("click", () => step(1));
    // The chip's own state is the key's to flip (see `wireKey`); the search only
    // looks again once it has.
    chips.forEach((button) => button.addEventListener("click", run));

    // A panel that arrives while a search is running has to be searched too, and
    // one that was set again has had its marks replaced along with its markup.
    // Looking again over the whole column is both the simplest way to be right
    // and what a reader would do: the count is of the folio, not of what it held
    // when they typed.
    //
    // The reader's *place* in that count is theirs, though, so it is kept rather
    // than reset: a session grows at its end, so the hit they were on is the hit
    // at the same ordinal, and a folio that shrank below it lands them on the
    // last one. Nothing here scrolls or opens a fold, because nobody asked it to.
    return {
      again() {
        if (!input.value) return;
        const held = index;
        mark();
        index = hits.length
          ? Math.min(Math.max(held, 0), hits.length - 1)
          : -1;
        paint(false);
      },
    };
  };

  // --- The nib: a quill's scratch as a copy is taken ---------------------
  //
  // Synthesized rather than sampled, for the same reason everything else here
  // is inlined: a folio carries every byte it needs, and a recording of a pen
  // would cost tens of kilobytes where this costs a few lines. A nib doesn't
  // glide over parchment, it catches and releases dozens of times a stroke,
  // and that stick-slip is what an ear hears as a scratch rather than a hiss,
  // so the noise is cut into grains that each bite their own amount, and then
  // trimmed to the band a dry point on paper actually sounds in. What is
  // written is a word rather than a mark: strokes of their own length, weight,
  // and tone, scheduled one after another with the pen lifted between them.

  let quill = null;

  // Built on the first copy rather than at load: a context made with no
  // gesture behind it starts suspended, and browsers count it against the page.
  const nib = () => {
    if (quill) return quill;
    const Context = window.AudioContext || window.webkitAudioContext;
    if (!Context) return null;
    try {
      quill = new Context();
    } catch {
      return null;
    }
    return quill;
  };

  // One pull of the nib: how long it is down, how hard it is pressed, how
  // dark it sounds, how coarsely it catches, and how long the pen is off the
  // page after. A word is a handful of these, and no two of them match: a
  // stem is a flick where a bowl is a long pull, the hand leans in and eases
  // off, and the nib's angle changes what each one sounds like. Skewing the
  // length keeps most strokes short, so the occasional long one tells.
  const strokesOfAWord = () => {
    const strokes = [];
    const letters = 5 + Math.floor(Math.random() * 4);
    for (let n = 0; n < letters; n += 1) {
      strokes.push({
        down: 0.018 + Math.random() ** 1.6 * 0.1,
        // No lift after the last: the word ends when the pen does.
        lifted: n === letters - 1 ? 0 : 0.015 + Math.random() * 0.05,
        press: 0.35 + Math.random() * 0.65,
        floor: 600 + Math.random() * 800,
        ceiling: 2400 + Math.random() * 2400,
        catches: 0.0004 + Math.random() * 0.0035,
      });
    }
    return strokes;
  };

  // One stroke, sounded at `when`: grains of noise under the pressure of a
  // hand, cut at either end to the band this pull sounds in. Below is a rumble
  // the pen doesn't have; above is the sizzle that makes the same grains read
  // as static rather than as a nib.
  const drawStroke = (pen, stroke, when) => {
    const length = Math.floor(pen.sampleRate * stroke.down);
    const buffer = pen.createBuffer(1, length, pen.sampleRate);
    const samples = buffer.getChannelData(0);
    let caught = 0;
    let bite = 0;
    for (let i = 0; i < length; i += 1) {
      if (i >= caught) {
        caught = i + pen.sampleRate * stroke.catches * (0.5 + Math.random());
        bite = 0.25 + Math.random() * 0.75;
      }
      // The stroke's own pressure: on as the hand commits, off as it lifts, so
      // every stroke ends at silence and the gap after it is a clean one.
      const weight = Math.sin((Math.PI * i) / length) ** 0.7;
      samples[i] = (Math.random() * 2 - 1) * bite * weight * stroke.press;
    }
    const source = pen.createBufferSource();
    source.buffer = buffer;
    const rumble = pen.createBiquadFilter();
    rumble.type = "highpass";
    rumble.frequency.value = stroke.floor;
    const sizzle = pen.createBiquadFilter();
    sizzle.type = "lowpass";
    sizzle.frequency.value = stroke.ceiling;
    sizzle.Q.value = 0.9;
    const quiet = pen.createGain();
    // Most strokes are pressed well under full, so the word wants a little
    // more gain than one flat stroke would to land as loudly.
    quiet.gain.value = 0.07;
    source
      .connect(rumble)
      .connect(sizzle)
      .connect(quiet)
      .connect(pen.destination);
    source.start(when);
  };

  const scratch = () => {
    const pen = nib();
    if (!pen) return;
    // resume() is a promise, and it rejects where the browser won't let the
    // context start. The word simply goes unheard, which is not worth an
    // unhandled rejection surfacing on a page whose copy already worked.
    if (pen.state === "suspended") pen.resume().catch(() => {});
    // A lead on the first stroke, so scheduling the word can't run late into
    // its own opening.
    let when = pen.currentTime + 0.01;
    for (const stroke of strokesOfAWord()) {
      drawStroke(pen, stroke, when);
      when += stroke.down + stroke.lifted;
    }
  };

  // --- Copy: a button on every code block and every message --------------

  const copyToClipboard = async (text) => {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // file:// or a denied permission: fall back to a throwaway textarea.
    }
    const scratch = document.createElement("textarea");
    scratch.value = text;
    scratch.style.position = "fixed";
    scratch.style.opacity = "0";
    document.body.appendChild(scratch);
    scratch.select();
    try {
      document.execCommand("copy");
    } catch {}
    scratch.remove();
  };

  const makeCopyButton = (getText) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "copy-button";
    button.textContent = "copy";
    button.setAttribute("aria-label", "copy to clipboard");
    // A hover is the reader announcing the click a moment early, so the pen is
    // readied then: building the context is the one costly step, and doing it
    // here leaves the click with only the stroke to draw. It starts suspended,
    // since a hover carries no user gesture to unblock audio, and the click
    // that follows resumes it. Once, because the context outlives the hover.
    button.addEventListener("pointerenter", nib, { once: true });
    button.addEventListener("click", async (event) => {
      event.stopPropagation();
      // The copy goes first and the pen follows in the same click. The hover
      // above usually spares this click the context, but a reader who reached
      // the button by keyboard pays tens of milliseconds to build one here, and
      // the clipboard is what they pressed the button for.
      const copied = copyToClipboard(getText());
      scratch();
      await copied;
      button.textContent = "copied";
      button.classList.add("is-done");
      setTimeout(() => {
        button.textContent = "copy";
        button.classList.remove("is-done");
      }, 1200);
    });
    return button;
  };

  // Seats a copy button on everything within `root` that can be copied. Called
  // once over the whole column, and again over each panel that arrives while the
  // folio is open, so a pushed panel is as copyable as one that was there from
  // the start.
  const wireCopy = (root) => {
    const container = root || document.querySelector("main.folio");
    if (!container) return null;

    // Every code / diff / JSON / output block copies its own text.
    container.querySelectorAll("pre").forEach((pre) => {
      const code = pre.querySelector("code") || pre;
      pre.appendChild(makeCopyButton(() => code.textContent));
    });

    // A fold whose body is prose carries no `pre` to hang a button on, so it
    // gets its own. This is the most load-bearing text in a folio to be able to
    // lift out: a skill's whole instructions, a rule pulled into context, a
    // plan, the prompt a subagent was sent. The text is taken before the button
    // is seated, since afterwards it would be inside what it copies.
    container.querySelectorAll(".tool--prose").forEach((prose) => {
      const text = prose.textContent.trim();
      prose.appendChild(makeCopyButton(() => text));
    });

    // Every turn copies its readable prose (text and thinking, not tool JSON).
    // The root itself when it is a panel that just arrived, since `querySelectorAll`
    // looks below a node and never at it.
    const turns = container.matches(".turn")
      ? [container]
      : container.querySelectorAll(".turn");
    turns.forEach((turn) => {
      const prose = turn.querySelectorAll(".block--text, .block--thinking");
      if (!prose.length) return;
      const meta = turn.querySelector(".turn__meta");
      if (!meta) return;
      const button = makeCopyButton(() =>
        Array.from(prose)
          .map((block) => block.textContent.trim())
          .filter(Boolean)
          .join("\n\n"),
      );
      button.classList.add("copy-button--meta");
      meta.appendChild(button);
    });
    return { adopt: wireCopy };
  };

  // --- Dock: jump between messages, fold every marginalia ---------------

  // How far down the viewport a panel still counts as the one being read: enough
  // to clear a turn's own scroll-margin, so the panel just navigated to is
  // current rather than the one before it.
  const THRESHOLD = 40;

  const wireDock = () => {
    const dock = document.querySelector(".dock");
    const container = document.querySelector("main.folio");
    if (!dock || !container) return null;
    // The unscoped stores these replaced imposed one folio's state on every
    // other; drop them rather than leave them to sit unread forever.
    try {
      localStorage.removeItem(FOLDS);
      localStorage.removeItem(TAIL);
    } catch {}
    // Step along the folio's own axis: what reached the model, and what it
    // produced. `data-side` carries the classification the renderer already
    // holds (see `PanelKind::side`), so this need not keep a list of kinds that
    // would drift from it. The `aside` kinds are skipped by both arrows and by
    // the unscoped middle pair: a plan boundary, a rule, and a passing note are
    // context a reader reaches for rather than stops at, and stepping to every
    // one of them would make the dock no faster than scrolling. Only visible
    // panels, since one a reader has no way to see reports top 0 and would
    // hijack "current".
    // The key narrows this the same way it narrows the search: an arrow steps
    // through the kinds a reader left in play on its own side, so turning off
    // `tool` and `thinking` leaves the warm arrow walking replies alone. The
    // `aside` kinds stay out of the dock however the key is set, since the
    // arrows are the two sides and those kinds are on neither.
    const messages = (side) => {
      const kinds = enabledKinds();
      return Array.from(
        container.querySelectorAll(
          side
            ? `.turn[data-side="${side}"]`
            : '.turn[data-side="entered"], .turn[data-side="model"]',
        ),
      ).filter(
        (turn) =>
          turn.getClientRects().length > 0 && kinds.has(turn.dataset.kind),
      );
    };

    // Land on a panel: name it in the URL, and scroll to its start. Every way of
    // arriving at a panel goes through here, so a folio's URL always names where
    // its reader is, whether they stepped with the dock, leapt to an end, or
    // scrubbed the minimap.
    //
    // `replaceState` rather than assigning `location.hash`, so twenty steps
    // don't cost twenty presses of Back; it performs no scroll of its own, hence
    // the explicit one, which honours the turn's `scroll-margin-top`.
    //
    // The panel is marked as well as named, because `:target` answers to
    // navigation and `replaceState` is not navigation: the URL changes and the
    // browser's own idea of the target does not follow it. The gilt wash that
    // says "you landed here" would otherwise appear only when a reader arrived
    // by a link, and stay stuck on that panel through every step after. The
    // stylesheet draws the mark and `:target` the same way, so a landing and an
    // arrival read alike.
    const marked = () => container.querySelectorAll("[data-landed]");

    const name = (target) => {
      marked().forEach((panel) => delete panel.dataset.landed);
      target.dataset.landed = "";
      try {
        history.replaceState(null, "", `#${target.id}`);
      } catch {
        location.hash = `#${target.id}`;
      }
    };

    const landOn = (target, marking = true) => {
      releaseTail();
      if (marking) name(target);
      target.scrollIntoView({ behavior: "auto", block: "start" });
    };

    const jump = (direction, side) => {
      const panels = messages(side);
      const tops = panels.map((turn) => turn.getBoundingClientRect().top);
      const target = panels[core.stepIndex(tops, direction, THRESHOLD)];
      if (!target) return;
      // Step by the turn's own permalink, and land at once rather than gliding.
      // Both follow from the same thing: a served folio changes under the reader,
      // so a smooth scroll still in flight can be left heading somewhere that has
      // moved, and where it was heading is not recorded anywhere. Writing the hash
      // makes the URL name where the reader is, which the deep-link handler below
      // already restores on the next load, and an instant landing is what
      // stepping through a folio wants in any case: an animation is a thing to
      // wait out when the reader means to press the button again.
      landOn(target);
    };

    const fold = (open) => {
      container.querySelectorAll("details").forEach((details) => {
        details.open = open;
      });
    };

    // --- Follow (tail -f): keep the newest message's start pinned as panels
    // arrive, and across a reload, until the reader scrolls away.
    //
    // Only a followed folio is told when its session grows, so only it carries
    // the toggle, and its presence is what says following is possible here. A
    // written one is a snapshot of a session that may have ended long ago:
    // there is nothing to follow, so jumping to its end is just a jump.
    const tailButton = dock.querySelector('[data-tail="toggle"]');
    const canFollow = Boolean(tailButton);

    const visible = () =>
      Array.from(container.querySelectorAll(".turn")).filter(
        (turn) => turn.getClientRects().length > 0,
      );

    const firstMessage = () => visible()[0] || null;

    const lastMessage = () => {
      const panels = visible();
      return panels[panels.length - 1] || null;
    };

    // What following remembers is not a flag but the permalink it last wrote:
    // the mode and where it had reached, in one value rather than two that could
    // disagree. The pin is what tells a hash the folio wrote itself apart from
    // one the reader arrived with, which a flag alone cannot do: a live session
    // grows, so by the time a followed folio is reloaded the hash it wrote no
    // longer names the end.
    let pinned = canFollow ? stored(perFolio(TAIL)) : null;
    let tailing = Boolean(pinned);

    const paintTail = () => {
      if (tailButton) tailButton.setAttribute("aria-pressed", String(tailing));
    };

    // Following is a mode, not a jump: while it is on, the newest panel's
    // permalink is the folio's to write, and it is rewritten every time the end
    // moves. It moves whenever a panel arrives and whenever the leaf reflows
    // beneath the reader (a fold opening, the web fonts landing). The URL
    // therefore keeps naming the turn the reader is actually on, so a reload
    // resumes at the end and a link copied out of a followed folio names what was
    // on the screen.
    // Every landing the dock makes is instant, the leaps to either end
    // included: a folio is megabytes tall, so a glide from one end to the other
    // is an animation to sit through rather than a sense of where you went, and a
    // panel arriving under a scroll still in flight moves what it was heading
    // for. (Stepping between search hits still glides, deliberately: those are
    // short hops within a page the reader is already looking at, where the
    // movement is what shows the next match is near.)
    const scrollToEnd = () => {
      const target = lastMessage();
      if (!target) return;
      name(target);
      if (tailing) {
        pinned = target.id;
        try {
          localStorage.setItem(perFolio(TAIL), pinned);
        } catch {}
      }
      target.scrollIntoView({ behavior: "auto", block: "start" });
    };

    const scrollToTop = () => {
      const target = firstMessage();
      if (!target) return;
      name(target);
      target.scrollIntoView({ behavior: "auto", block: "start" });
    };

    // While following, the folio decides where a reload lands, so the browser
    // must not: left on "auto" it restores the scroll position it recorded
    // before the reload, asynchronously and after this script has run, quietly
    // undoing the snap to the end. A followed folio is reloaded whenever the
    // server restarts under it, and a live session has moved on by then, which is
    // exactly when the two disagree.
    const holdScroll = () => {
      try {
        history.scrollRestoration = tailing ? "manual" : "auto";
      } catch {}
    };

    // Turning it on pins the end (`scrollToEnd` records where); turning it off
    // forgets, so the absence of a pin is the absence of the mode.
    const setTail = (on) => {
      tailing = canFollow && on;
      if (!tailing) {
        pinned = null;
        try {
          localStorage.removeItem(perFolio(TAIL));
        } catch {}
      }
      holdScroll();
      paintTail();
      if (on) scrollToEnd();
    };

    // A programmatic scrollIntoView never emits wheel/touch/keydown, so those
    // are unambiguous reader input: any of them hands control back.
    const releaseTail = () => {
      if (tailing) setTail(false);
    };
    window.addEventListener("wheel", releaseTail, { passive: true });
    window.addEventListener("touchmove", releaseTail, { passive: true });
    window.addEventListener("keydown", (event) => {
      const tag = event.target.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "BUTTON") return;
      if (SCROLL_KEYS.has(event.key)) releaseTail();
    });

    // A deep link (#turn-N) names the panel the reader came for, so it releases
    // follow like a wheel or arrow key does, rather than losing the landing to
    // a snap to the end. Releasing (not just skipping this load's snap) because
    // the hash survives a reload, and a suppression that didn't persist would
    // fight the anchor on every one.
    //
    // Unless it is the hash following itself last wrote, which is the folio
    // naming where the reader is rather than the reader naming where they want
    // to be. That is what the pin is for: reading every hash as the reader's
    // meant following survived exactly one reload, and none at all once a step
    // of the dock had left a permalink in the URL.
    //
    // The hash need not be a valid selector either, hence getElementById, as
    // querySelector throws on one that isn't.
    const anchoredPanel = () => {
      const anchored = location.hash
        ? document.getElementById(core.anchorId(location.hash))
        : null;
      return anchored && container.contains(anchored) ? anchored : null;
    };
    const named = anchoredPanel();
    const deepLink =
      named && core.readersHash(location.hash, pinned) ? named : null;
    if (deepLink) releaseTail();

    // Following a turn's own number is the reader naming where they are, so it
    // hands control back the way a scroll does. The folio's own writes go
    // through `replaceState`, which fires nothing here, and the fallback path
    // records the pin before the event can run.
    window.addEventListener("hashchange", () => {
      // A real navigation, so the browser's own `:target` takes over the mark:
      // leaving one behind would wash two panels at once.
      marked().forEach((panel) => delete panel.dataset.landed);
      if (core.readersHash(location.hash, pinned)) releaseTail();
    });

    // On load, if still following, snap to the newest message at once; a second
    // pass after layout settles (web fonts shift it) lands it precisely. A
    // deep-linked turn needs that second pass too, since the browser's own
    // anchor scroll happens before the fonts land.
    paintTail();
    holdScroll();
    // Keep the end pinned as the leaf changes height under the reader: a panel
    // arrives, the fonts land, an image decodes, a fold is opened. Each moves the
    // newest panel's start, and following means being there rather than where it
    // used to be. Watching whether or not this load began by following, since the
    // reader can turn it on at any point after.
    if (canFollow) {
      new ResizeObserver(() => {
        if (tailing) scrollToEnd();
      }).observe(container);
    }
    if (tailing) {
      scrollToEnd();
      // Again on the next frame, and again once every resource is in: a folio
      // is megabytes of markup and its layout is still settling long after this
      // script runs, and each settling moves the end.
      requestAnimationFrame(() => {
        if (tailing) scrollToEnd();
      });
      window.addEventListener("load", () => {
        if (tailing) scrollToEnd();
      });
    } else if (deepLink) {
      requestAnimationFrame(() => {
        deepLink.scrollIntoView({ behavior: "auto", block: "start" });
      });
    }

    dock.addEventListener("click", (event) => {
      const button = event.target.closest("button");
      if (!button) return;
      const { nav, side, fold: foldTo, tail } = button.dataset;
      if (nav === "prev") jump(-1, side);
      else if (nav === "next") jump(1, side);
      // Leaping to the top is the reader taking control, so it releases follow
      // the way a wheel or arrow key does: otherwise the next panel to arrive
      // would snap them straight back to the end.
      else if (nav === "top") {
        releaseTail();
        scrollToTop();
      } else if (nav === "end") setTail(true);
      else if (tail === "toggle") setTail(!tailing);
      else if (foldTo === "expand") fold(true);
      else if (foldTo === "collapse") fold(false);
    });

    // Remember each marginalia's open/closed state, keyed per message so a live
    // session that grows keeps the folds a reader set: the raw stream is
    // append-only, so a panel's turn number and a marginalia's index within it
    // stay stable as new turns arrive. That is what lets a fold survive both a
    // reload and its own panel being set again when a tool result joins it. Only
    // open keys are stored; the markup default is collapsed.
    const foldKey = (details) => {
      const turn = details.closest(".turn");
      const marginalia = turn ? turn.querySelectorAll("details") : [details];
      const index = Array.prototype.indexOf.call(marginalia, details);
      return core.foldKey(turn && turn.dataset.turn, index);
    };

    const readOpenFolds = () => {
      try {
        const held = JSON.parse(stored(perFolio(FOLDS)) || "[]");
        return new Set(Array.isArray(held) ? held : []);
      } catch {
        return new Set();
      }
    };

    // A panel that arrives while the folio is open is restored the same way, so a
    // fold the reader opened in a panel that is then set again (a tool result
    // joining its call does exactly that) comes back open rather than shut.
    const restoreFolds = (root) => {
      const open = readOpenFolds();
      root.querySelectorAll("details").forEach((details) => {
        if (open.has(foldKey(details))) details.open = true;
      });
    };

    restoreFolds(container);

    // `toggle` fires on both a reader's click and the fold-all buttons, and does
    // not bubble, so listen in the capture phase.
    container.addEventListener(
      "toggle",
      (event) => {
        const details = event.target;
        if (!(details instanceof HTMLDetailsElement)) return;
        const folds = readOpenFolds();
        if (details.open) folds.add(foldKey(details));
        else folds.delete(foldKey(details));
        try {
          localStorage.setItem(perFolio(FOLDS), JSON.stringify([...folds]));
        } catch {}
      },
      true,
    );

    // `landOn` is handed to the minimap, which is another way of arriving at a
    // panel and so must arrive the same way: releasing follow, naming the turn,
    // landing at its start. Passed rather than reached for, so the two agree by
    // construction instead of by a second copy of this.
    //
    // The rest is what a panel arriving on a followed folio needs: its folds
    // restored, and the end pinned again if the reader is there. The end moves
    // whenever the leaf does, which the ResizeObserver above already catches, but
    // saying so here means the landing happens with the panel rather than a frame
    // after it.
    return {
      landOn,
      adopt: restoreFolds,
      toEnd() {
        if (tailing) scrollToEnd();
      },
    };
  };

  // --- Minimap: the whole folio at a glance, and a place to scrub it -----

  // A hairline under a band, so a one-line note is still somewhere to aim at in
  // a folio whose tool output runs to thousands of lines, and a thicker one
  // under the reader's own view, which a long folio otherwise draws as a pixel.
  const BAND_FLOOR = 2;
  const VIEW_FLOOR = 12;

  // How fast the wheel zooms the map, and how far in it can go. The rate is per
  // pixel of wheel delta and exponential, so a notch is the same proportion of a
  // zoom wherever it is turned; the limit is generous because the folios that
  // want zoom are the ones with a thousand panels in them.
  const ZOOM_RATE = 0.002;
  const MOST_ZOOM = 64;

  const wireMinimap = (dock) => {
    const minimap = document.querySelector(".minimap");
    const container = document.querySelector("main.folio");
    if (!minimap || !container || !dock) return null;
    const track = minimap.querySelector(".minimap__track");
    const view = minimap.querySelector(".minimap__view");
    // No test for a panel: a folio that has none yet is exactly the one that is
    // about to be sent some (`serve` on a session in its first seconds), and a
    // map that was never wired can never adopt them. An empty map simply draws
    // no bands.
    if (!track || !view) return null;

    // A band per panel, in the panel's own pigment. Drawn from the panels
    // themselves rather than written into the markup: what a band states is the
    // share of the document its panel takes, which only the browser knows and
    // which changes every time a fold opens.
    //
    // Drawn again from scratch whenever the panels change, for the same reason:
    // a band stands for a panel, so a folio that gained one has a map that is
    // missing it, and a map missing a stretch of the document misstates where
    // everything else in it sits.
    let bands = [];
    const draw = () => {
      track.querySelectorAll(".minimap__band").forEach((band) => band.remove());
      bands = Array.from(container.querySelectorAll(".turn")).map((panel) => {
        const band = document.createElement("div");
        band.className = "minimap__band";
        band.dataset.kind = panel.dataset.kind || "";
        band.dataset.turn = panel.dataset.turn || "";
        if (panel.dataset.sidechain !== undefined) band.dataset.sidechain = "";
        band.title = `#${panel.dataset.turn} ${panel.dataset.kind}`;
        track.insertBefore(band, view);
        return { band, panel };
      });
    };
    draw();

    // The whole scrollable page rather than the folio's own extent, so the
    // reader's view sits on the map where it sits on the document.
    const leaf = () => document.documentElement.scrollHeight || 1;

    // The stretch of the leaf the track is showing. Zoomed in, the map stops
    // being the whole folio and becomes a part of it, which is the point: a
    // session of a thousand panels draws most of them two pixels tall, and two
    // pixels is a mark rather than a target.
    //
    // How it was last framed is a fact about this folio, so it is remembered
    // under this folio and survives a reload: a reader who has opened up the
    // stretch they are working through keeps it.
    const framed = core.framing(stored(perFolio(MAP)));
    let lens = core.lens({
      leaf: leaf(),
      track: track.clientHeight,
      zoom: framed.zoom,
    });
    lens = core.lens({ ...lens, origin: framed.at * lens.leaf });

    const remember = () => {
      // An unzoomed map is the whole folio, which is where every map starts:
      // nothing to remember, so nothing is kept.
      try {
        if (lens.zoom <= 1) localStorage.removeItem(perFolio(MAP));
        else {
          localStorage.setItem(
            perFolio(MAP),
            JSON.stringify({ zoom: lens.zoom, at: lens.origin / lens.leaf }),
          );
        }
      } catch {}
    };

    // Re-measured whenever the leaf or the track changes size. Following the
    // reader is *not* part of that: zoom is the map's own, so looking into one
    // stretch of a folio while reading another is exactly what it is for. Only
    // the reader's own scrolling brings the map back to them.
    const relens = (follow) => {
      lens = core.lens({ ...lens, leaf: leaf(), track: track.clientHeight });
      if (follow) lens = core.followed(lens, window.scrollY, window.innerHeight);
      // Zoomed, the map stands for a part of the folio rather than the whole of
      // it, and the stylesheet says so.
      if (lens.zoom > 1) minimap.dataset.zoomed = "";
      else delete minimap.dataset.zoomed;
    };

    const layout = () => {
      bands.forEach(({ band, panel }) => {
        const box = panel.getBoundingClientRect();
        band.hidden = box.height === 0;
        if (band.hidden) return;
        const placed = core.bandBox(
          box.top + window.scrollY,
          box.height,
          lens,
          BAND_FLOOR,
        );
        band.style.top = `${placed.top}px`;
        band.style.height = `${placed.height}px`;
      });
    };

    const paintView = () => {
      const placed = core.viewBox(
        window.scrollY,
        window.innerHeight,
        lens,
        VIEW_FLOOR,
      );
      view.style.top = `${placed.top}px`;
      view.style.height = `${placed.height}px`;
    };

    // The key narrows the map as it narrows the search and the dock: a kind out
    // of play is still drawn, since the map would otherwise misstate where
    // everything else sits, but it goes faint and is no longer somewhere the
    // scrub can land.
    const paintKinds = () => {
      const kinds = enabledKinds();
      bands.forEach(({ band }) => {
        band.dataset.inPlay = String(kinds.has(band.dataset.kind));
      });
    };

    const nearest = (clientY) => {
      const y = clientY - track.getBoundingClientRect().top;
      const index = core.nearestIndex(
        y,
        bands.map(({ band }) => ({
          top: band.offsetTop,
          height: band.offsetHeight,
          inPlay: !band.hidden && band.dataset.inPlay === "true",
        })),
      );
      return index === -1 ? null : bands[index].panel;
    };

    let scrubbing = false;
    let landed = null;

    // The permalink is written when the scrub settles, not at every panel it
    // passes over: a drag crosses dozens, and the browsers that throttle
    // `replaceState` count them all.
    const scrub = (event, settled) => {
      const target = nearest(event.clientY);
      if (!target || (target === landed && !settled)) return;
      landed = target;
      dock.landOn(target, settled);
    };

    track.addEventListener("pointerdown", (event) => {
      scrubbing = true;
      track.setPointerCapture(event.pointerId);
      scrub(event, false);
      event.preventDefault();
    });

    track.addEventListener("pointermove", (event) => {
      if (scrubbing) scrub(event, false);
    });

    const settle = (event) => {
      if (!scrubbing) return;
      scrubbing = false;
      landed = null;
      scrub(event, true);
    };

    track.addEventListener("pointerup", settle);
    track.addEventListener("pointercancel", settle);

    // Everything the map draws is a frame's worth of work over every panel, and
    // a wheel or a scroll arrives many times a frame, so a redraw is asked for
    // rather than done.
    let asked = false;
    const redraw = (follow) => {
      if (asked) return;
      asked = true;
      requestAnimationFrame(() => {
        asked = false;
        relens(follow);
        layout();
        paintView();
      });
    };

    window.addEventListener("scroll", () => redraw(true), { passive: true });

    // Zoom, on the map alone: the wheel over the track narrows what it shows
    // instead of scrolling the leaf, so a reader can open up a stretch of a
    // thousand-panel folio where every band is two pixels tall, and pick one
    // out, without leaving the place they are reading. Held about the pointer,
    // as a map zooms about the cursor.
    //
    // The wheel is stopped here rather than let through: it must not scroll the
    // page under the reader, and it must not reach the window listener that
    // takes following as released, since looking at the map is not the reader
    // leaving the end of the session.
    track.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault();
        event.stopPropagation();
        lens = core.zoomedAbout(
          lens,
          event.clientY - track.getBoundingClientRect().top,
          Math.exp(-event.deltaY * ZOOM_RATE),
          MOST_ZOOM,
        );
        relens(false);
        layout();
        paintView();
        remember();
      },
      { passive: false },
    );

    // Every height the map states can change without a scroll: a fold opening,
    // the web fonts landing, the window resizing. Observing the folio catches
    // all three, and fires once on its own to draw the map in the first place.
    // None of them is the reader moving, so none of them brings the map back to
    // where they are.
    new ResizeObserver(() => redraw(false)).observe(container);
    window.addEventListener("resize", () => redraw(false));
    const key = document.querySelector(".key");
    if (key) key.addEventListener("click", paintKinds);
    paintKinds();
    redraw(true);

    return {
      adopt() {
        draw();
        paintKinds();
        redraw(false);
      },
    };
  };

  // --- Following a session as it is written ------------------------------
  //
  // A served page names the stream it is told changes on (`data-live`), and its
  // presence is the whole of what says this page can be told: a written folio
  // names none, so nothing here runs and nothing waits.
  //
  // What arrives is markup, panel by panel, keyed by the turn number the panel
  // carries as its id. So a change is applied by replacing that panel and nothing
  // else: the reader keeps their scroll position, their open folds, their search,
  // their theme, and their place in the conversation, none of which survives the
  // reload this replaced.
  //
  // The markup is the scribe's own, escaped as everything it writes is, and it
  // arrives over the same connection the page did. `insertAdjacentHTML` and
  // `template` both refuse to run a script in it, so a panel is set exactly as it
  // would have been had the page been loaded with it.

  const parsePanel = (html) => {
    const held = document.createElement("template");
    held.innerHTML = html;
    return held.content.firstElementChild;
  };

  // Markup that arrives can set a character the cut faces dropped, whether it is
  // a panel of transcript or a session title in a listing, so what arrives says
  // which faces it needs and the page is re-dressed before it is seated: swapped
  // after, the text is drawn once in a face that lacks it.
  const dress = (href) => {
    if (!href) return;
    const link = document.querySelector("link[data-faces]");
    if (link && !link.href.endsWith(href)) link.href = href;
  };

  const wireLive = (parts) => {
    const stream = document.body.dataset.live;
    const container = document.querySelector("main.folio");
    if (!stream) return;

    let boot = null;

    const seatPanel = (turn, html) => {
      const arriving = parsePanel(html);
      if (!arriving) return;
      const standing = Array.from(container.querySelectorAll(".turn"));
      const held = standing.find((panel) => panel.dataset.turn === String(turn));
      if (held) {
        held.replaceWith(arriving);
      } else {
        const turns = standing.map((panel) => Number(panel.dataset.turn));
        const at = core.seatFor(turns, Number(turn));
        container.insertBefore(arriving, standing[at] || null);
      }
      parts.copy?.adopt(arriving);
      parts.dock?.adopt(arriving);
    };

    const applyPanels = (told) => {
      if (!container) return;
      dress(told.faces);
      (told.gone || []).forEach((turn) => {
        const held = container.querySelector(`.turn[data-turn="${turn}"]`);
        if (held) held.remove();
      });
      (told.panels || []).forEach(({ turn, html }) => seatPanel(turn, html));
      if (told.facts) {
        const facts = document.querySelector(".plaque__facts");
        if (facts) facts.replaceWith(parsePanel(told.facts));
      }
      // The map stands for the panels, and the search counts them, so both look
      // again once the leaf is settled rather than once per panel.
      parts.minimap?.adopt();
      parts.search?.again();
      parts.dock?.toEnd();
    };

    const source = new EventSource(stream);

    source.addEventListener("hello", (event) => {
      const said = JSON.parse(event.data).boot;
      if (core.restarted(boot, said)) {
        location.reload();
        return;
      }
      boot = said;
    });

    source.addEventListener("panels", (event) => {
      applyPanels(JSON.parse(event.data));
    });

    // A listing is a few kilobytes and every row of it moves as time passes, so
    // it is replaced whole rather than picked apart.
    source.addEventListener("listing", (event) => {
      const told = JSON.parse(event.data);
      dress(told.faces);
      const shelf = document.querySelector("[data-listing]");
      if (shelf) shelf.innerHTML = told.html;
    });
  };

  const onReady = (fn) => {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", fn);
    } else {
      fn();
    }
  };

  onReady(() => {
    // The key first: it restores which kinds are in play, and everything below
    // reads that as it wires itself.
    wireKey();
    wireThemeToggle();
    const search = wireSearch();
    const copy = wireCopy();
    const dock = wireDock();
    const minimap = wireMinimap(dock);
    // Last, because what arrives has to be handed to every part that has already
    // wired itself against the panels.
    wireLive({ search, copy, dock, minimap });
  });
})();
